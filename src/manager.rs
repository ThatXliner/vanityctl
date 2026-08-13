use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use sysinfo::System;

use crate::{
    backend::BackendSet,
    config::{ConfigPaths, HostConfig, expand_path},
    deploy::DeployCoordinator,
    dns::{DnsReconciler, DnsStatus},
    model::{DeploymentRecord, Event, JobRun, RuntimeState, Service, ServiceStatus, ServiceType},
    runner::{SharedRunner, SystemRunner},
    state::StateStore,
};

pub struct Manager {
    pub paths: ConfigPaths,
    runner: SharedRunner,
    backends: BackendSet,
    state: Arc<StateStore>,
    deployer: DeployCoordinator,
    dns: DnsReconciler,
    system: Mutex<System>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub changed: Vec<String>,
    pub unchanged: Vec<String>,
    pub errors: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub healthy: bool,
    pub version: String,
    pub os: String,
    pub hostname: String,
    pub config: String,
    pub resources: HostResources,
    pub checks: Vec<DoctorCheck>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResources {
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub gpu_percent: Option<f64>,
    pub gpu_memory_bytes: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl Manager {
    pub fn system(paths: ConfigPaths) -> Result<Self> {
        Self::new(paths, Arc::new(SystemRunner))
    }
    pub fn new(paths: ConfigPaths, runner: SharedRunner) -> Result<Self> {
        paths.ensure_runtime_dirs()?;
        let state = Arc::new(StateStore::load(&paths)?);
        Ok(Self {
            backends: BackendSet::new(runner.clone(), paths.clone()),
            deployer: DeployCoordinator::new(runner.clone(), state.clone(), paths.clone()),
            dns: DnsReconciler::new(state.clone()),
            system: Mutex::new(System::new_all()),
            paths,
            runner,
            state,
        })
    }
    pub fn config(&self) -> Result<HostConfig> {
        HostConfig::load(&self.paths)
    }
    fn service(&self, name: &str) -> Result<Service> {
        self.config()?
            .services
            .get(name)
            .cloned()
            .with_context(|| format!("unknown service {name:?}; run `vanityctl list`"))
    }

    pub async fn statuses(&self) -> Result<Vec<ServiceStatus>> {
        let config = self.config()?;
        let snapshot = self.state.snapshot();
        let mut statuses = Vec::new();
        for (name, service) in &config.services {
            let mut effective = service.clone();
            if let Some(value) = snapshot.services.get(name).and_then(|s| s.enabled_override) {
                effective.enabled = value;
            }
            let mut status = self
                .backends
                .get(&service.kind)
                .status(name, &effective)
                .await
                .unwrap_or_else(|error| {
                    let mut value = ServiceStatus {
                        name: name.clone(),
                        kind: service.kind.clone(),
                        state: RuntimeState::Unknown,
                        health: None,
                        uptime_seconds: None,
                        cpu_percent: None,
                        memory_bytes: None,
                        pid: None,
                        ports: service.ports.clone(),
                        details: Some(error.to_string()),
                        deployment: None,
                        latest_job: None,
                    };
                    value.health = Some("error".into());
                    value
                });
            if let Some(state) = snapshot.services.get(name) {
                if service.source.is_some() {
                    status.deployment = Some(state.deployment.clone());
                }
                status.latest_job = state.job_runs.last().cloned();
            }
            statuses.push(status);
        }
        Ok(statuses)
    }
    pub async fn status(&self, name: &str) -> Result<ServiceStatus> {
        self.statuses()
            .await?
            .into_iter()
            .find(|s| s.name == name)
            .with_context(|| format!("unknown service {name:?}"))
    }

    pub async fn apply(&self) -> Result<ApplyResult> {
        let config = self.config()?;
        let mut result = ApplyResult {
            changed: vec![],
            unchanged: vec![],
            errors: BTreeMap::new(),
        };
        for (name, service) in &config.services {
            let outcome = self.ensure_source(service);
            if let Err(error) = outcome {
                result.errors.insert(name.clone(), format!("{error:#}"));
                continue;
            }
            match self.backends.get(&service.kind).apply(name, service).await {
                Ok(true) => result.changed.push(name.clone()),
                Ok(false) => result.unchanged.push(name.clone()),
                Err(error) => {
                    result.errors.insert(name.clone(), format!("{error:#}"));
                }
            }
            self.state
                .update(|s| s.services.entry(name.clone()).or_default().enabled_override = None)?;
            if let Some(source) = &service.source {
                let auto = service
                    .deploy
                    .as_ref()
                    .map(|deploy| deploy.auto)
                    .unwrap_or(false);
                self.state.update(|s| {
                    let state = s.services.entry(name.clone()).or_default();
                    state.auto_deploy_override = None;
                    state.deployment.auto = auto;
                    state.deployment.branch = source.branch.clone();
                })?;
            }
        }
        self.state.event(
            "apply",
            None,
            format!(
                "apply finished: {} changed, {} errors",
                result.changed.len(),
                result.errors.len()
            ),
        )?;
        Ok(result)
    }

    fn ensure_source(&self, service: &Service) -> Result<()> {
        let Some(source) = &service.source else {
            return Ok(());
        };
        let directory = expand_path(
            service
                .directory
                .as_deref()
                .context("Git-backed service requires directory")?,
        )?;
        if directory.join(".git").exists() {
            return Ok(());
        }
        if directory.exists() && fs::read_dir(&directory)?.next().is_some() {
            bail!("{} exists and is not empty", directory.display());
        }
        if let Some(parent) = directory.parent() {
            fs::create_dir_all(parent)?;
        }
        self.runner.run(
            "git",
            &[
                "clone".into(),
                "--branch".into(),
                source.branch.clone(),
                source.repo.clone(),
                directory.display().to_string(),
            ],
            None,
        )?;
        Ok(())
    }

    pub async fn action(&self, name: &str, action: &str) -> Result<()> {
        let service = self.service(name)?;
        let backend = self.backends.get(&service.kind);
        match action {
            "start" => backend.start(name, &service).await?,
            "stop" => backend.stop(name, &service).await?,
            "restart" => backend.restart(name, &service).await?,
            _ => bail!("unsupported action {action}"),
        }
        self.state
            .event("lifecycle", Some(name), format!("{action} requested"))?;
        Ok(())
    }
    pub async fn logs(&self, name: &str, lines: usize) -> Result<String> {
        let service = self.service(name)?;
        self.backends
            .get(&service.kind)
            .logs(name, &service, lines)
            .await
    }
    pub async fn compose_operation(&self, name: &str, operation: &str) -> Result<()> {
        let service = self.service(name)?;
        if service.kind != ServiceType::Compose {
            bail!("service {name} is not a compose service");
        }
        let backend = self.backends.get(&service.kind);
        match operation {
            "pull" => backend.pull(name, &service).await,
            "build" => backend.build(name, &service).await,
            _ => bail!("unsupported compose operation {operation}"),
        }
    }
    pub async fn deploy(&self, name: &str, trigger: &str, retry: bool) -> Result<DeploymentRecord> {
        let service = self.service(name)?;
        self.deployer
            .deploy(
                name,
                &service,
                self.backends.get(&service.kind),
                trigger,
                retry,
            )
            .await
    }
    pub fn deployment_history(&self, name: &str) -> Result<Vec<DeploymentRecord>> {
        self.service(name)?;
        Ok(self
            .state
            .snapshot()
            .services
            .get(name)
            .map(|s| s.deployments.clone())
            .unwrap_or_default())
    }
    pub fn deployment_log(&self, name: &str, id: Option<&str>) -> Result<String> {
        let history = self.deployment_history(name)?;
        let record = match id {
            Some(id) => history.iter().find(|r| r.id == id),
            None => history.last(),
        }
        .context("deployment not found")?;
        Ok(fs::read_to_string(&record.log_file)?)
    }

    pub fn describe(&self, name: &str) -> Result<Value> {
        let service = self.service(name)?;
        let mut value = serde_json::to_value(&service)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("name".into(), json!(name));
            object.insert(
                "environment".into(),
                json!(service.environment.keys().collect::<Vec<_>>()),
            );
            if service.env_file.is_some() {
                object.insert("envFile".into(), json!("configured (value hidden)"));
            }
            if service.kind == ServiceType::Compose {
                let directory = expand_path(service.directory.as_deref().unwrap())?;
                let files = service.compose_files();
                object.remove("file");
                object.insert("files".into(), json!(files));
                object.insert(
                    "resolvedFiles".into(),
                    json!(
                        service
                            .compose_files()
                            .into_iter()
                            .map(|file| crate::config::resolve_compose_file(&directory, file))
                            .collect::<Result<Vec<_>>>()?
                    ),
                );
            }
        }
        Ok(value)
    }
    pub fn list(&self) -> Result<Vec<Value>> {
        Ok(self.config()?.services.iter().map(|(name, service)| json!({"name":name,"type":service.kind,"enabled":service.enabled,"description":service.description})).collect())
    }

    pub async fn run_job(&self, name: &str) -> Result<JobRun> {
        let service = self.service(name)?;
        if service.kind != ServiceType::Job {
            bail!("service {name} is not a job");
        }
        let command = expand_path(service.command.as_deref().unwrap())?;
        let cwd = service.directory.as_deref().map(expand_path).transpose()?;
        let started_at = Utc::now();
        let timer = Instant::now();
        let output = self.runner.run(
            command.to_str().context("job command is not valid UTF-8")?,
            &service.args,
            cwd.as_deref(),
        );
        let log_path = self
            .paths
            .logs
            .join("jobs")
            .join(name)
            .join(format!("{}.log", started_at.format("%Y%m%dT%H%M%S")));
        fs::create_dir_all(log_path.parent().unwrap())?;
        let (exit_code, body) = match output {
            Ok(out) => (out.code, format!("{}{}", out.stdout, out.stderr)),
            Err(error) => (1, format!("{error:#}\n")),
        };
        fs::write(&log_path, body)?;
        let run = JobRun {
            started_at,
            duration_ms: timer.elapsed().as_millis() as u64,
            exit_code,
            log_file: log_path.display().to_string(),
        };
        self.state.update(|state| {
            state
                .services
                .entry(name.into())
                .or_default()
                .job_runs
                .push(run.clone())
        })?;
        self.state.event(
            "job",
            Some(name),
            format!("job finished with exit code {exit_code}"),
        )?;
        Ok(run)
    }
    pub fn job_history(&self, name: &str) -> Result<Vec<JobRun>> {
        let svc = self.service(name)?;
        if svc.kind != ServiceType::Job {
            bail!("service {name} is not a job");
        }
        Ok(self
            .state
            .snapshot()
            .services
            .get(name)
            .map(|s| s.job_runs.clone())
            .unwrap_or_default())
    }
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let mut service = self.service(name)?;
        if service.kind != ServiceType::Job {
            bail!("enable/disable is supported for scheduled jobs");
        }
        service.enabled = enabled;
        self.backends
            .get(&service.kind)
            .apply(name, &service)
            .await?;
        self.state.update(|s| {
            s.services.entry(name.into()).or_default().enabled_override = Some(enabled)
        })?;
        self.state.event(
            "job",
            Some(name),
            format!(
                "job {} (runtime override; apply restores YAML)",
                if enabled { "enabled" } else { "disabled" }
            ),
        )?;
        Ok(())
    }
    pub fn set_auto_deploy(&self, name: &str, enabled: bool) -> Result<()> {
        let service = self.service(name)?;
        if service.source.is_none() {
            bail!("service {name} has no Git source");
        }
        self.state.update(|s| {
            let state = s.services.entry(name.into()).or_default();
            state.auto_deploy_override = Some(enabled);
            state.deployment.auto = enabled;
        })?;
        self.state.event(
            "deployment",
            Some(name),
            format!(
                "auto-deploy {} (runtime override; apply restores YAML)",
                if enabled { "enabled" } else { "disabled" }
            ),
        )
    }

    pub async fn dns_status(&self) -> Result<DnsStatus> {
        let config = self.config()?.dns.context("DNS is not configured")?;
        self.dns.status(&config).await
    }
    pub async fn dns_reconcile(&self) -> Result<DnsStatus> {
        let config = self.config()?.dns.context("DNS is not configured")?;
        self.dns.reconcile(&config).await
    }
    pub fn events(&self) -> Vec<Event> {
        self.state.snapshot().events
    }

    pub fn doctor(&self) -> DoctorReport {
        let mut checks = vec![];
        checks.push(check_command(&self.runner, "git", &["--version"]));
        checks.push(check_command(&self.runner, "docker", &["info"]));
        #[cfg(target_os = "macos")]
        checks.push(check_command(&self.runner, "launchctl", &["version"]));
        checks.push(match self.config() {
            Ok(_) => DoctorCheck {
                name: "configuration".into(),
                ok: true,
                detail: format!("{} is valid", self.paths.config.display()),
            },
            Err(e) => DoctorCheck {
                name: "configuration".into(),
                ok: false,
                detail: format!("{e:#}"),
            },
        });
        let healthy = checks.iter().all(|c| c.ok);
        DoctorReport {
            healthy,
            version: env!("CARGO_PKG_VERSION").into(),
            os: std::env::consts::OS.into(),
            hostname: hostname(),
            config: self.paths.config.display().to_string(),
            resources: self.host_resources(),
            checks,
        }
    }
    fn host_resources(&self) -> HostResources {
        let mut system = self
            .system
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        system.refresh_cpu_usage();

        #[cfg(target_os = "macos")]
        let (gpu_percent, gpu_memory_bytes) = self
            .runner
            .run(
                "ioreg",
                &[
                    "-r".into(),
                    "-c".into(),
                    "AGXAccelerator".into(),
                    "-l".into(),
                    "-w".into(),
                    "0".into(),
                ],
                None,
            )
            .ok()
            .map(|output| parse_apple_gpu_metrics(&output.stdout))
            .unwrap_or((None, None));
        #[cfg(not(target_os = "macos"))]
        let (gpu_percent, gpu_memory_bytes) = (None, None);

        HostResources {
            cpu_percent: system.global_cpu_usage() as f64,
            memory_used_bytes: system.used_memory(),
            memory_total_bytes: system.total_memory(),
            gpu_percent,
            gpu_memory_bytes,
        }
    }
    pub fn agent_context(&self) -> Result<String> {
        let config = self.config()?;
        let mut out = String::from(
            "# vanityctl machine context\n\nAll managed workloads must be operated through vanityctl. Do not manually kill managed processes or replace managed containers.\n\n## Services\n",
        );
        for (name, svc) in config.services {
            out.push_str(&format!(
                "- `{name}` ({:?}): {}\n",
                svc.kind,
                svc.description.unwrap_or_else(|| "no description".into())
            ));
        }
        out.push_str("\nUseful commands: `vanityctl status --json`, `vanityctl describe <service> --json`, `vanityctl logs <service>`, `vanityctl restart <service>`, `vanityctl deploy <service>`.\n");
        Ok(out)
    }

    pub fn start_auto_deployers(self: &Arc<Self>) -> Result<()> {
        for (name, service) in self.config()?.services {
            if service.source.is_none() {
                continue;
            }
            let interval = DeployCoordinator::polling_interval(&service)
                .unwrap_or(std::time::Duration::from_secs(60));
            let manager = self.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let result = async {
                        let current = manager.service(&name)?;
                        let snapshot = manager.state.snapshot();
                        let configured = current.deploy.as_ref().map(|d| d.auto).unwrap_or(false);
                        let enabled = snapshot
                            .services
                            .get(&name)
                            .and_then(|s| s.auto_deploy_override)
                            .unwrap_or(configured);
                        if !enabled {
                            return anyhow::Ok(());
                        }
                        let remote = manager.deployer.remote_commit(&current)?;
                        let local = snapshot
                            .services
                            .get(&name)
                            .and_then(|s| s.deployment.deployed_commit.as_deref());
                        let failed = snapshot
                            .services
                            .get(&name)
                            .and_then(|s| s.failed_commit.as_deref());
                        if local != Some(&remote) && failed != Some(&remote) {
                            manager.deploy(&name, "git-poll", false).await?;
                        }
                        anyhow::Ok(())
                    }
                    .await;
                    if let Err(error) = result {
                        let _ = manager.state.event(
                            "deployment",
                            Some(&name),
                            format!("poll failed: {error:#}"),
                        );
                    }
                }
            });
        }
        Ok(())
    }

    pub fn start_dns_reconciler(self: &Arc<Self>) -> Result<()> {
        let Some(dns) = self.config()?.dns else {
            return Ok(());
        };
        let interval = crate::config::parse_duration(&dns.interval)?;
        let manager = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Err(error) = manager.dns_reconcile().await {
                    let message = format!("DNS reconciliation failed: {error:#}");
                    let _ = manager
                        .state
                        .update(|state| state.dns_error = Some(message.clone()));
                    let _ = manager.state.event("dns", None, message);
                }
            }
        });
        Ok(())
    }
}

fn check_command(runner: &SharedRunner, name: &str, args: &[&str]) -> DoctorCheck {
    match runner.run(
        name,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
    ) {
        Ok(out) => DoctorCheck {
            name: name.into(),
            ok: true,
            detail: out.stdout.lines().next().unwrap_or("available").into(),
        },
        Err(error) => DoctorCheck {
            name: name.into(),
            ok: false,
            detail: error.to_string(),
        },
    }
}
fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn parse_apple_gpu_metrics(output: &str) -> (Option<f64>, Option<u64>) {
    let value_after = |key: &str| {
        output.split(key).nth(1).and_then(|tail| {
            tail.split_once('=')
                .map(|(_, value)| value)
                .and_then(|value| {
                    value
                        .trim_start()
                        .split(|c: char| !c.is_ascii_digit() && c != '.')
                        .find(|part| !part.is_empty())
                })
                .and_then(|value| value.parse::<f64>().ok())
        })
    };
    (
        value_after("Device Utilization %"),
        value_after("In use system memory\"").map(|value| value as u64),
    )
}

#[cfg(test)]
mod metrics_tests {
    use super::parse_apple_gpu_metrics;

    #[test]
    fn parses_apple_gpu_utilization_and_memory() {
        let input = r#""PerformanceStatistics" = {"In use system memory"=1007599616,"Device Utilization %"=71}"#;
        assert_eq!(
            parse_apple_gpu_metrics(input),
            (Some(71.0), Some(1_007_599_616))
        );
    }
}
