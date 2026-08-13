use std::{collections::HashMap, convert::Infallible, sync::Arc, time::Duration};

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::Manager;

#[derive(Clone)]
struct ApiState {
    manager: Arc<Manager>,
}
#[derive(Clone)]
struct AuthState {
    token: Option<String>,
    public_read_only: bool,
}

pub async fn serve(manager: Arc<Manager>) -> Result<()> {
    let config = manager.config()?;
    let listen = config.api.listen.clone();
    let token = config.api.resolve_token()?;
    let public_read_only = config.api.public_read_only;
    manager.start_auto_deployers()?;
    manager.start_dns_reconciler()?;
    let state = ApiState { manager };
    let routes = Router::new()
        .route("/", get(dashboard))
        .route("/api/services", get(list_services))
        .route("/api/status", get(statuses))
        .route("/api/services/{name}", get(describe))
        .route("/api/services/{name}/status", get(status))
        .route("/api/services/{name}/logs", get(logs))
        .route("/api/services/{name}/start", post(start))
        .route("/api/services/{name}/stop", post(stop))
        .route("/api/services/{name}/restart", post(restart))
        .route("/api/services/{name}/pull", post(pull))
        .route("/api/services/{name}/build", post(build))
        .route("/api/services/{name}/deploy", post(deploy))
        .route("/api/services/{name}/deploy/auto-enable", post(auto_enable))
        .route(
            "/api/services/{name}/deploy/auto-disable",
            post(auto_disable),
        )
        .route("/api/services/{name}/deployments", get(deployments))
        .route("/api/services/{name}/deployments/logs", get(deployment_log))
        .route("/api/jobs", get(jobs))
        .route("/api/jobs/{name}/run", post(run_job))
        .route("/api/jobs/{name}/history", get(job_history))
        .route("/api/jobs/{name}/enable", post(enable_job))
        .route("/api/jobs/{name}/disable", post(disable_job))
        .route("/api/apply", post(apply))
        .route("/api/apply/plan", get(apply_plan))
        .route("/api/plugins", get(plugins))
        .route("/api/plugins/library", get(plugin_library))
        .route("/api/plugins/{name}", get(plugin))
        .route("/api/config/validate", get(validate))
        .route("/api/system", get(system))
        .route("/api/events", get(events))
        .route("/api/events/stream", get(event_stream))
        .route("/api/agent-context", get(agent_context))
        .route("/api/dns", get(dns_status))
        .route("/api/dns/reconcile", post(dns_reconcile))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            AuthState {
                token,
                public_read_only,
            },
            authenticate,
        ));
    let listener = TcpListener::bind(&listen).await?;
    tracing::info!(%listen, "hostd listening");
    axum::serve(listener, routes)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

async fn authenticate(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    // The dashboard shell has no machine data. Public read-only mode opens only
    // this explicit list; configuration, logs, agent context, and every write
    // remain token-protected.
    if request.uri().path() == "/" {
        return next.run(request).await;
    }
    let Some(token) = auth.token else {
        return next.run(request).await;
    };
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if supplied == Some(token.as_str()) || (auth.public_read_only && is_public_read_route(&request))
    {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"missing or invalid bearer token"})),
        )
            .into_response()
    }
}

fn is_public_read_route(request: &axum::extract::Request) -> bool {
    if request.method() != axum::http::Method::GET {
        return false;
    }
    let path = request.uri().path();
    matches!(
        path,
        "/api/services"
            | "/api/status"
            | "/api/jobs"
            | "/api/system"
            | "/api/events"
            | "/api/events/stream"
            | "/api/dns"
    ) || path
        .strip_prefix("/api/services/")
        .and_then(|tail| tail.strip_suffix("/status"))
        .is_some_and(|name| !name.is_empty() && !name.contains('/'))
}

struct ApiError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(value: E) -> Self {
        Self(value.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":format!("{:#}", self.0)})),
        )
            .into_response()
    }
}

async fn list_services(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.list()?)))
}
async fn plugins(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.plugins()?)))
}
async fn plugin_library(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.manager.plugin_library()))
}
async fn plugin(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.plugin(&name)?)))
}
async fn statuses(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.statuses().await?)))
}
async fn describe(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.manager.describe(&name)?))
}
async fn status(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.status(&name).await?)))
}

#[derive(Deserialize)]
struct LogsQuery {
    #[serde(default = "default_lines")]
    lines: usize,
}
fn default_lines() -> usize {
    200
}
async fn logs(
    State(s): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<LogsQuery>,
) -> Result<Response, ApiError> {
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        s.manager.logs(&name, q.lines.min(10_000)).await?,
    )
        .into_response())
}
async fn start(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.action(&name, "start").await?;
    Ok(Json(json!({"ok":true})))
}
async fn stop(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.action(&name, "stop").await?;
    Ok(Json(json!({"ok":true})))
}
async fn restart(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.action(&name, "restart").await?;
    Ok(Json(json!({"ok":true})))
}
async fn pull(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.compose_operation(&name, "pull").await?;
    Ok(Json(json!({"ok":true})))
}
async fn build(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.compose_operation(&name, "build").await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct DeployQuery {
    #[serde(default)]
    retry: bool,
}
async fn deploy(
    State(s): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<DeployQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(
        s.manager.deploy(&name, "manual", q.retry).await?
    )))
}
async fn auto_enable(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.set_auto_deploy(&name, true)?;
    Ok(Json(json!({"ok":true})))
}
async fn auto_disable(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.set_auto_deploy(&name, false)?;
    Ok(Json(json!({"ok":true})))
}
async fn deployments(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.deployment_history(&name)?)))
}
async fn deployment_log(
    State(s): State<ApiState>,
    Path(name): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        s.manager
            .deployment_log(&name, q.get("id").map(String::as_str))?,
    )
        .into_response())
}

async fn jobs(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let statuses = s.manager.statuses().await?;
    Ok(Json(json!(
        statuses
            .into_iter()
            .filter(|x| matches!(x.kind, crate::model::ServiceType::Job))
            .collect::<Vec<_>>()
    )))
}
async fn run_job(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.run_job(&name).await?)))
}
async fn job_history(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.job_history(&name)?)))
}
async fn enable_job(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.set_enabled(&name, true).await?;
    Ok(Json(json!({"ok":true})))
}
async fn disable_job(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.manager.set_enabled(&name, false).await?;
    Ok(Json(json!({"ok":true})))
}
async fn apply(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.apply().await?)))
}
async fn apply_plan(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.apply_plan()?)))
}
async fn validate(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let c = s.manager.config()?;
    Ok(Json(
        json!({"valid":true,"version":c.version,"services":c.services.len(),"plugins":c.resolved_plugins.len()}),
    ))
}
async fn system(State(s): State<ApiState>) -> Json<Value> {
    Json(json!(s.manager.doctor()))
}
async fn events(State(s): State<ApiState>) -> Json<Value> {
    Json(json!(s.manager.events()))
}
async fn agent_context(State(s): State<ApiState>) -> Result<Response, ApiError> {
    Ok((
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        s.manager.agent_context()?,
    )
        .into_response())
}
async fn dns_status(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.dns_status().await?)))
}
async fn dns_reconcile(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.manager.dns_reconcile().await?)))
}

async fn event_stream(
    State(s): State<ApiState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold((s.manager, 0usize), |(manager, seen)| async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let events = manager.events();
        let payload = serde_json::to_string(&events.iter().skip(seen).collect::<Vec<_>>()).unwrap();
        let next = events.len();
        Some((
            Ok(Event::default().event("activity").data(payload)),
            (manager, next),
        ))
    });
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD)
}

const DASHBOARD: &str = r##"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>vanityctl</title><style>
:root{color-scheme:dark;--bg:#0b0d10;--panel:#14181e;--panel2:#0f1217;--muted:#8993a4;--line:#282f39;--green:#54d68b;--red:#ff667d;--amber:#ffc857;--blue:#7ab7ff}*{box-sizing:border-box}[hidden]{display:none!important}body{margin:0;background:var(--bg);color:#edf1f7;font:14px ui-monospace,SFMono-Regular,Menlo,monospace}header{position:sticky;top:0;z-index:5;display:flex;justify-content:space-between;align-items:center;padding:19px 28px;background:rgba(11,13,16,.94);border-bottom:1px solid var(--line);backdrop-filter:blur(12px)}h1{font-size:18px;margin:0}.slash{color:var(--muted)}h2{font-size:15px;margin:28px 0 12px}.wrap{max-width:1440px;margin:auto;padding:28px}.summary{color:var(--muted);margin-bottom:18px}.resource-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-bottom:22px}.resource{padding:14px 16px;background:var(--panel);border:1px solid var(--line);border-radius:9px}.resource strong{display:block;font-size:18px;margin-top:7px}.panel{background:var(--panel);border:1px solid var(--line);border-radius:9px;overflow:hidden}.service-head,.service-row{display:grid;grid-template-columns:minmax(180px,2fr) minmax(130px,1.2fr) 105px minmax(100px,1fr) 90px 100px 190px;gap:14px;align-items:center}.service-head{padding:10px 16px;color:var(--muted);font-size:11px;text-transform:uppercase;letter-spacing:.08em;background:var(--panel2);border-bottom:1px solid var(--line)}.service-row{min-height:58px;padding:11px 16px;border-bottom:1px solid var(--line)}.service-row:last-child{border-bottom:0}.service-row:hover{background:#181d24}.name-link,.back{border:0;background:none;color:#edf1f7;padding:0;font:inherit;font-weight:700;text-align:left}.name-link:hover,.back:hover{color:var(--blue);background:none}.muted{color:var(--muted)}.running,.synced{color:var(--green)}.failed,.unknown{color:var(--red)}.stopped,.idle{color:var(--amber)}button{background:#232a34;color:#fff;border:1px solid #394352;border-radius:6px;padding:7px 10px;cursor:pointer;font:inherit}button:hover{background:#303947}.secondary{background:transparent}.row-actions,.detail-actions,.toolbar{display:flex;gap:8px;align-items:center;justify-content:flex-end}.detail-header{display:flex;justify-content:space-between;gap:20px;align-items:flex-start;margin:18px 0 22px}.detail-header h2{font-size:24px;margin:8px 0 4px}.tabs{display:flex;gap:18px;border-bottom:1px solid var(--line);margin-bottom:18px}.tab{border:0;border-radius:0;background:none;color:var(--muted);padding:10px 2px;border-bottom:2px solid transparent}.tab:hover{background:none;color:#fff}.tab.active{color:#fff;border-bottom-color:var(--green)}.detail-panel{background:var(--panel);border:1px solid var(--line);border-radius:9px;padding:20px}.facts{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:1px;background:var(--line);border:1px solid var(--line);border-radius:8px;overflow:hidden}.fact{background:var(--panel);padding:16px}.fact span{display:block;color:var(--muted);font-size:12px;margin-bottom:7px}.fact strong{font-size:15px}.toolbar{justify-content:flex-start;margin-bottom:12px}.toolbar input{min-width:240px;flex:1;max-width:460px;background:#090b0e;color:#fff;border:1px solid var(--line);border-radius:6px;padding:8px 10px;font:inherit}.log-output,.config-output{white-space:pre-wrap;overflow:auto;background:#090b0e;border:1px solid #20262e;padding:16px;border-radius:8px;margin:0}.log-output{height:min(62vh,680px)}.config-output{max-height:65vh}.activity{padding:16px 18px}.activity div+div{margin-top:9px}.notice{color:var(--muted);padding:18px}.error{color:var(--red)}dialog{width:min(420px,calc(100vw - 32px));background:var(--panel);color:#fff;border:1px solid var(--line);border-radius:10px;padding:22px}dialog::backdrop{background:rgba(0,0,0,.72)}dialog h2{font-size:18px;margin:0 0 8px}dialog p{color:var(--muted);line-height:1.5}dialog input{width:100%;background:#090b0e;color:#fff;border:1px solid var(--line);border-radius:6px;padding:10px;font:inherit;margin:8px 0 16px}@media(max-width:1000px){.service-head,.service-row{grid-template-columns:minmax(160px,2fr) minmax(125px,1fr) 100px 90px 180px}.col-signal,.col-memory{display:none}.facts{grid-template-columns:repeat(2,1fr)}}@media(max-width:700px){header{padding:16px}.wrap{padding:18px}.host-meta{display:none}.resource-grid{grid-template-columns:1fr}.service-head{display:none}.service-row{grid-template-columns:1fr auto;gap:8px}.service-row>.col-type,.service-row>.col-state,.service-row>.col-signal,.service-row>.col-cpu,.service-row>.col-memory{display:none}.row-actions{grid-column:2}.facts{grid-template-columns:1fr}.detail-header{display:block}.detail-actions{justify-content:flex-start;margin-top:16px}.toolbar{flex-wrap:wrap}.toolbar input{min-width:100%;order:2}}
</style></head>
<body><header><h1>vanityctl <span class="slash">/</span> <span id="machineName">host</span></h1><span id="hostMeta" class="muted host-meta">loading…</span></header>
<main class="wrap">
  <section id="dashboardView">
    <div id="summary" class="summary"></div><div id="resources" class="resource-grid"></div>
    <div class="panel"><div class="service-head"><span>Service</span><span>Workload</span><span>State</span><span>Health / deploy</span><span>CPU</span><span>RAM</span><span></span></div><div id="services"><div class="notice">Loading services…</div></div></div>
    <section id="dnsSection" hidden><h2>DNS</h2><div id="dns" class="detail-panel"></div></section>
    <h2>Recent activity</h2><div id="activity" class="panel activity muted">No activity yet.</div>
  </section>
  <section id="serviceView" hidden>
    <button class="back" onclick="closeService()">← All services</button>
    <div class="detail-header"><div><h2 id="detailName"></h2><div id="detailType" class="muted"></div></div><div id="detailActions" class="detail-actions"></div></div>
    <nav class="tabs" aria-label="Service details"><button id="tabOverview" class="tab active" onclick="showTab('overview')">Overview</button><button id="tabLogs" class="tab" onclick="showTab('logs')">Logs</button><button id="tabConfig" class="tab" onclick="showTab('config')">Configuration</button></nav>
    <div id="overviewPanel" class="detail-panel"><div class="notice">Loading status…</div></div>
    <div id="logsPanel" class="detail-panel" hidden><div class="toolbar"><button id="pauseLogs" onclick="toggleLogs()">Pause</button><button onclick="loadLogs()">Refresh</button><button onclick="downloadLogs()">Download</button><input id="logFilter" type="search" placeholder="Filter logs" oninput="renderLogs()" aria-label="Filter logs"></div><pre id="logOutput" class="log-output">Loading logs…</pre></div>
    <div id="configPanel" class="detail-panel" hidden><pre id="configOutput" class="config-output">Loading configuration…</pre></div>
  </section>
</main>
<dialog id="authDialog"><form onsubmit="submitToken(event)"><h2>Unlock controls</h2><p>Public access is read-only. Enter the hostd API token to view logs and configuration or operate this service.</p><label for="tokenInput" class="muted">API token</label><input id="tokenInput" type="password" autocomplete="current-password" required><div class="detail-actions"><button type="button" class="secondary" onclick="cancelToken()">Cancel</button><button type="submit">Unlock</button></div></form></dialog>
<script>
const esc=s=>String(s??'—').replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[c]));
const bytes=n=>{if(n==null)return '—';const u=['B','KiB','MiB','GiB','TiB'];let i=0;while(n>=1024&&i<u.length-1){n/=1024;i++}return n.toFixed(i>1?1:0)+' '+u[i]};
const duration=s=>{if(s==null)return '—';const d=Math.floor(s/86400),h=Math.floor(s%86400/3600),m=Math.floor(s%3600/60);return[d&&d+'d',h&&h+'h',m+'m'].filter(Boolean).join(' ')};
const workloadType=t=>({compose:['Docker Compose','An existing multi-container Docker Compose project'],docker:['Docker container','A single container managed directly by vanityctl'],process:['Native process','A long-running process supervised by the operating system'],job:['Scheduled job','A task run on a schedule or on demand']}[t]||[t,'Workload type']);
let token=sessionStorage.getItem('vanityctl-token'),selected=null,currentTab='overview',rawLogs='',logsPaused=false,authPromise=null,authResolve=null;
function requestToken(){if(authPromise)return authPromise;authPromise=new Promise(resolve=>{authResolve=resolve;tokenInput.value='';authDialog.showModal();setTimeout(()=>tokenInput.focus(),0)});return authPromise}
function finishToken(value){authDialog.close();const resolve=authResolve;authPromise=null;authResolve=null;resolve(value)}
function submitToken(event){event.preventDefault();token=tokenInput.value.trim();if(!token)return;sessionStorage.setItem('vanityctl-token',token);finishToken(token)}
function cancelToken(){finishToken(null)}
async function api(path,opt={},retry=true){opt.headers={...(opt.headers||{}),...(token?{Authorization:'Bearer '+token}:{})};const r=await fetch('/api'+path,opt);if(r.status===401&&retry){token=null;sessionStorage.removeItem('vanityctl-token');if(await requestToken())return api(path,opt,false)}if(!r.ok)throw Error((await r.json()).error||r.statusText);return (r.headers.get('content-type')||'').includes('json')?r.json():r.text()}
function fact(label,value,klass=''){return `<div class="fact"><span>${esc(label)}</span><strong class="${klass}">${esc(value)}</strong></div>`}
async function load(){try{const [sys,rows,events]=await Promise.all([api('/system'),api('/status'),api('/events')]);machineName.textContent=sys.hostname.split('.')[0];hostMeta.textContent=sys.os+' · v'+sys.version;summary.textContent=rows.length+' services · '+rows.filter(x=>x.state==='running').length+' running · '+rows.filter(x=>x.health==='error'||x.state==='failed').length+' need attention';const r=sys.resources||{};resources.innerHTML=`<div class="resource"><span class="muted">CPU</span><strong>${r.cpuPercent==null?'—':esc(r.cpuPercent.toFixed(1)+'%')}</strong></div><div class="resource"><span class="muted">RAM</span><strong>${esc(bytes(r.memoryUsedBytes))} <span class="muted">/ ${esc(bytes(r.memoryTotalBytes))}</span></strong></div><div class="resource"><span class="muted">GPU</span><strong>${r.gpuPercent==null?'unavailable':esc(r.gpuPercent.toFixed(1)+'%')} ${r.gpuMemoryBytes==null?'':`<span class="muted">· ${esc(bytes(r.gpuMemoryBytes))}</span>`}</strong></div>`;services.innerHTML=rows.map(row=>`<div class="service-row"><button class="name-link" onclick="openService('${esc(row.name)}','${esc(row.type)}')">${esc(row.name)}</button><span class="muted col-type" title="${esc(workloadType(row.type)[1])}">${esc(workloadType(row.type)[0])}</span><span class="${esc(row.state)} col-state">● ${esc(row.state)}</span><span class="col-signal ${row.health==='error'?'failed':'muted'}">${esc(row.health||(row.deployment?.status)||(row.latestJob?.exitCode===0?'last run ✓':'—'))}</span><span class="muted col-cpu">${row.cpuPercent==null?'—':esc(row.cpuPercent.toFixed(1)+'%')}</span><span class="muted col-memory">${row.memoryBytes==null?'—':esc(bytes(row.memoryBytes))}</span><span class="row-actions"><button class="secondary" onclick="openService('${esc(row.name)}','${esc(row.type)}')">Details</button><button onclick="act('${esc(row.name)}','${row.type==='job'?'run':'restart'}')">${row.type==='job'?'Run':'Restart'}</button></span></div>`).join('')||'<div class="notice">No services configured.</div>';activity.innerHTML=events.slice(-8).reverse().map(e=>`<div>${esc(new Date(e.timestamp).toLocaleTimeString())} · ${esc(e.message)}</div>`).join('')||'No activity yet.';try{const d=await api('/dns');dnsSection.hidden=false;dns.innerHTML=`Public IP: ${esc(d.publicIp)} · ${d.records.filter(r=>r.synced).length}/${d.records.length} synced <button onclick="reconcileDns()">Reconcile now</button>`}catch(_){dnsSection.hidden=true}}catch(e){summary.textContent=e.message}}
async function openService(name,type){selected={name,type};dashboardView.hidden=true;serviceView.hidden=false;detailName.textContent=name;detailType.textContent=workloadType(type)[0]+' · '+workloadType(type)[1];detailActions.innerHTML=`<button onclick="act('${esc(name)}','${type==='job'?'run':'restart'}')">${type==='job'?'Run now':'Restart'}</button>`;showTab('overview');scrollTo(0,0)}
function closeService(){selected=null;serviceView.hidden=true;dashboardView.hidden=false;load();scrollTo(0,0)}
async function showTab(tab){currentTab=tab;for(const name of ['Overview','Logs','Config']){document.getElementById('tab'+name).classList.toggle('active',name.toLowerCase()===tab);document.getElementById(name.toLowerCase()+'Panel').hidden=name.toLowerCase()!==tab}if(tab==='overview')await loadOverview();if(tab==='logs')await loadLogs();if(tab==='config')await loadConfig()}
async function loadOverview(){if(!selected)return;overviewPanel.innerHTML='<div class="notice">Loading status…</div>';try{const s=await api('/services/'+selected.name+'/status');const signal=s.health||s.deployment?.status||'—';overviewPanel.innerHTML=`<div class="facts">${fact('State',s.state,s.state)}${fact('Health / deploy',signal,s.health==='error'?'failed':'')}${fact('Uptime',duration(s.uptimeSeconds))}${fact('CPU',s.cpuPercent==null?'—':s.cpuPercent.toFixed(1)+'%')}${fact('RAM',bytes(s.memoryBytes))}${fact('PID',s.pid)}${fact('Ports',(s.ports||[]).join(', ')||'—')}${fact('Details',s.details||'—')}${fact('Last job',s.latestJob?.exitCode==null?'—':s.latestJob.exitCode===0?'Succeeded':'Failed ('+s.latestJob.exitCode+')')}</div>`}catch(e){overviewPanel.innerHTML=`<div class="error">${esc(e.message)}</div>`}}
async function loadLogs(){if(!selected||logsPaused)return;logOutput.textContent='Loading logs…';try{rawLogs=await api('/services/'+selected.name+'/logs?lines=1000');renderLogs()}catch(e){logOutput.textContent=e.message}}
function renderLogs(){const q=logFilter.value.toLowerCase();logOutput.textContent=q?rawLogs.split('\n').filter(line=>line.toLowerCase().includes(q)).join('\n'):rawLogs||'No logs yet.';logOutput.scrollTop=logOutput.scrollHeight}
function toggleLogs(){logsPaused=!logsPaused;pauseLogs.textContent=logsPaused?'Resume':'Pause';if(!logsPaused)loadLogs()}
function downloadLogs(){if(!selected)return;const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([rawLogs],{type:'text/plain'}));a.download=selected.name+'.log';a.click();URL.revokeObjectURL(a.href)}
async function loadConfig(){if(!selected)return;configOutput.textContent='Loading configuration…';try{configOutput.textContent=JSON.stringify(await api('/services/'+selected.name),null,2)}catch(e){configOutput.textContent=e.message}}
async function reconcileDns(){try{await api('/dns/reconcile',{method:'POST'});await load()}catch(e){alert(e.message)}}
async function act(name,action){try{await api((action==='run'?'/jobs/':'/services/')+name+'/'+action,{method:'POST'});if(selected)await loadOverview();else await load()}catch(e){alert(e.message)}}
load();setInterval(()=>{if(!selected)load();else if(currentTab==='overview')loadOverview();else if(currentTab==='logs'&&!logsPaused)loadLogs()},5000);
</script></body></html>"##;

#[cfg(test)]
mod auth_tests {
    use super::is_public_read_route;
    use axum::{body::Body, http::Request};

    fn request(method: &str, path: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn public_read_only_allowlist_excludes_secrets_and_writes() {
        for path in [
            "/api/services",
            "/api/status",
            "/api/services/web/status",
            "/api/jobs",
            "/api/system",
            "/api/events",
            "/api/events/stream",
            "/api/dns",
        ] {
            assert!(is_public_read_route(&request("GET", path)), "{path}");
        }
        for (method, path) in [
            ("GET", "/api/services/web"),
            ("GET", "/api/services/web/logs"),
            ("GET", "/api/agent-context"),
            ("POST", "/api/services/web/restart"),
            ("POST", "/api/jobs/backup/run"),
            ("POST", "/api/apply"),
            ("POST", "/api/dns/reconcile"),
        ] {
            assert!(!is_public_read_route(&request(method, path)), "{path}");
        }
    }
}
