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
}

pub async fn serve(manager: Arc<Manager>) -> Result<()> {
    let config = manager.config()?;
    let listen = config.api.listen.clone();
    let token = config
        .api
        .token_env
        .as_ref()
        .map(std::env::var)
        .transpose()?
        .filter(|v| !v.is_empty());
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
            AuthState { token },
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
    let Some(token) = auth.token else {
        return next.run(request).await;
    };
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if supplied == Some(token.as_str()) {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"missing or invalid bearer token"})),
        )
            .into_response()
    }
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
:root{color-scheme:dark;--bg:#0b0d10;--panel:#15191f;--muted:#8993a4;--line:#282f39;--green:#54d68b;--red:#ff667d;--amber:#ffc857}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:#edf1f7;font:14px ui-monospace,SFMono-Regular,Menlo,monospace}header{display:flex;justify-content:space-between;align-items:center;padding:22px 28px;border-bottom:1px solid var(--line)}h1{font-size:18px;margin:0}h2{font-size:15px;margin:28px 0 12px}.wrap{max-width:1200px;margin:auto;padding:26px}.summary{color:var(--muted);margin-bottom:18px}.grid{display:grid;gap:12px}.service{display:grid;grid-template-columns:2fr 1fr 1fr 1.4fr .7fr auto;gap:14px;align-items:center;padding:16px 18px;background:var(--panel);border:1px solid var(--line);border-radius:10px}.panel{padding:16px 18px;background:var(--panel);border:1px solid var(--line);border-radius:10px}.name{font-weight:700}.muted{color:var(--muted)}.running,.synced{color:var(--green)}.failed,.unknown{color:var(--red)}.stopped,.idle{color:var(--amber)}button{background:#232a34;color:#fff;border:1px solid #394352;border-radius:6px;padding:7px 10px;cursor:pointer}button:hover{background:#303947}dialog{width:min(850px,90vw);background:var(--panel);color:#fff;border:1px solid var(--line);border-radius:12px}pre{white-space:pre-wrap;max-height:55vh;overflow:auto;background:#090b0e;padding:16px;border-radius:8px}.actions{display:flex;gap:8px;justify-content:flex-end}@media(max-width:700px){.service{grid-template-columns:1fr 1fr}.hide-small{display:none}}</style></head>
<body><header><h1>vanityctl / this computer</h1><span id="host" class="muted">loading…</span></header><main class="wrap"><div id="summary" class="summary"></div><div id="services" class="grid"></div><section id="dnsSection" hidden><h2>DNS</h2><div id="dns" class="panel"></div></section><h2>Recent activity</h2><div id="activity" class="panel muted">No activity yet.</div></main><dialog id="detail"><div id="detailBody"></div><form method="dialog" class="actions"><button>Close</button></form></dialog>
<script>
const esc=s=>String(s??'—').replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[c]));
let token=sessionStorage.getItem('vanityctl-token');
async function api(path,opt={}){opt.headers={...(opt.headers||{}),...(token?{Authorization:'Bearer '+token}:{})};const r=await fetch('/api'+path,opt);if(r.status===401&&!token){token=prompt('hostd API token');if(token){sessionStorage.setItem('vanityctl-token',token);return api(path,opt)}}if(!r.ok)throw Error((await r.json()).error||r.statusText);return (r.headers.get('content-type')||'').includes('json')?r.json():r.text()}
async function load(){try{const [sys,rows,events]=await Promise.all([api('/system'),api('/status'),api('/events')]);host.textContent=sys.hostname+' · '+sys.os+' · v'+sys.version;summary.textContent=rows.length+' services · '+rows.filter(x=>x.state==='running').length+' running · '+rows.filter(x=>x.health==='error'||x.state==='failed').length+' need attention';services.innerHTML=rows.map(row=>`<div class="service" onclick="show('${esc(row.name)}')"><span class="name">${esc(row.name)}</span><span class="muted">${esc(row.type)}</span><span class="${esc(row.state)}">● ${esc(row.state)}</span><span class="hide-small muted">${esc(row.deployment?.status||(row.latestJob?.exitCode===0?'last run ✓':'—'))}</span><span class="hide-small muted">${row.cpuPercent==null?'—':esc(row.cpuPercent.toFixed(1)+'% CPU')}</span><span><button onclick="event.stopPropagation();act('${esc(row.name)}','${row.type==='job'?'run':'restart'}')">${row.type==='job'?'Run':'Restart'}</button></span></div>`).join('');activity.innerHTML=events.slice(-8).reverse().map(e=>`<div>${esc(new Date(e.timestamp).toLocaleTimeString())} · ${esc(e.message)}</div>`).join('')||'No activity yet.';try{const d=await api('/dns');dnsSection.hidden=false;dns.innerHTML=`<div>Public IP: ${esc(d.publicIp)} · ${d.records.filter(r=>r.synced).length}/${d.records.length} synced <button onclick="reconcileDns()">Reconcile now</button></div>`}catch(_){dnsSection.hidden=true}}catch(e){summary.textContent=e.message}}
async function reconcileDns(){try{await api('/dns/reconcile',{method:'POST'});await load()}catch(e){alert(e.message)}}
async function show(name){const [desc,status,logs]=await Promise.all([api('/services/'+name),api('/services/'+name+'/status'),api('/services/'+name+'/logs?lines=200')]);detailBody.innerHTML=`<h2>${esc(name)}</h2><pre>${esc(JSON.stringify({status,configuration:desc},null,2))}</pre><h3>Recent logs</h3><pre>${esc(logs)}</pre>`;detail.showModal()}
async function act(name,action){try{await api((action==='run'?'/jobs/':'/services/')+name+'/'+action,{method:'POST'});await load()}catch(e){alert(e.message)}}load();setInterval(load,5000);
</script></body></html>"##;
