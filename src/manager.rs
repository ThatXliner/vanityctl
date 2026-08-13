use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    backend::BackendSet,
    config::{ConfigPaths, HostConfig, expand_path},
    deploy::DeployCoordinator,
    dns::{DnsReconciler, DnsStatus},
    model::{DeploymentRecord, Event, JobRun, RuntimeState, Service, ServiceStatus, ServiceType},
    plugin::{PluginApplication, PluginResolution, application_directory, stdlib_catalog},
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
pub struct ApplyPlan {
    pub services: Vec<String>,
    pub plugins: Vec<PluginResolution>,
    pub actions: Vec<String>,
    pub note: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub healthy: bool,
    pub version: String,
    pub os: String,
    pub hostname: String,
    pub config: String,
    pub checks: Vec<DoctorCheck>,
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
        let mut unavailable_plugins = BTreeSet::new();
        for (instance, plugin) in &config.resolved_plugins {
            let Some(application) = &plugin.application else {
                continue;
            };
            if let Err(error) = self.materialize_plugin_source(instance, application) {
                result.errors.insert(instance.clone(), format!("{error:#}"));
                unavailable_plugins.insert(instance.clone());
            }
        }
        for (name, service) in &config.services {
            if service
                .generated_by
                .as_ref()
                .is_some_and(|plugin| unavailable_plugins.contains(&plugin.instance))
            {
                continue;
            }
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

    pub fn apply_plan(&self) -> Result<ApplyPlan> {
        let config = self.config()?;
        Ok(ApplyPlan {
            services: config.services.keys().cloned().collect(),
            plugins: config.resolved_plugins.values().cloned().collect(),
            actions: config
                .resolved_plugins
                .iter()
                .filter_map(|(name, plugin)| {
                    plugin.application.as_ref().map(|application| {
                        format!(
                            "materialize {name} from {}@{}",
                            application.repo, application.revision
                        )
                    })
                })
                .chain(
                    config
                        .services
                        .iter()
                        .map(|(name, service)| format!("reconcile {name} ({:?})", service.kind)),
                )
                .collect(),
            note: "dry run only; no service, scheduler, Docker, or launchd changes were made",
        })
    }

    pub fn plugins(&self) -> Result<Vec<PluginResolution>> {
        Ok(self.config()?.resolved_plugins.into_values().collect())
    }

    pub fn plugin(&self, name: &str) -> Result<PluginResolution> {
        self.config()?
            .resolved_plugins
            .remove(name)
            .with_context(|| format!("unknown plugin instance {name:?}"))
    }

    pub fn plugin_library(&self) -> Value {
        json!(stdlib_catalog())
    }

    pub fn materialize_plugin_sources(&self) -> Result<Vec<String>> {
        let config = self.config()?;
        let mut changed = Vec::new();
        for (instance, plugin) in &config.resolved_plugins {
            if let Some(application) = &plugin.application
                && self.materialize_plugin_source(instance, application)?
            {
                changed.push(instance.clone());
            }
        }
        Ok(changed)
    }

    fn materialize_plugin_source(
        &self,
        instance: &str,
        application: &PluginApplication,
    ) -> Result<bool> {
        let target = expand_path(&application.directory)?;
        let marker = self.plugin_source_marker(instance);
        if target.exists() && fs::read_dir(&target)?.next().is_some() {
            let recorded: PluginApplication = serde_json::from_slice(
                &fs::read(&marker).with_context(|| {
                    format!(
                        "plugin {instance}: {} is not empty and has no vanityctl ownership marker; move it aside or choose another directory",
                        target.display()
                    )
                })?,
            )?;
            if recorded.repo != application.repo
                || recorded.revision != application.revision
                || recorded.subdirectory != application.subdirectory
                || recorded.directory != application.directory
            {
                bail!(
                    "plugin {instance}: existing source ownership does not match the requested pinned source; vanityctl will not overwrite it"
                );
            }
            self.verify_plugin_checkout(instance, application, &target)?;
            return Ok(false);
        }

        let parent = target
            .parent()
            .context("plugin application directory has no parent")?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".vanityctl-{instance}-clone-{}",
            uuid::Uuid::new_v4()
        ));
        let clone_result = self.runner.run(
            "git",
            &[
                "clone".into(),
                "--no-checkout".into(),
                "--filter=blob:none".into(),
                "--".into(),
                application.repo.clone(),
                temporary.display().to_string(),
            ],
            None,
        );
        if let Err(error) = clone_result {
            remove_temporary(&temporary);
            return Err(error)
                .with_context(|| format!("plugin {instance}: clone application source"));
        }
        let checkout = self.runner.run(
            "git",
            &[
                "checkout".into(),
                "--detach".into(),
                application.revision.clone(),
            ],
            Some(&temporary),
        );
        if let Err(error) = checkout {
            remove_temporary(&temporary);
            return Err(error).with_context(|| {
                format!("plugin {instance}: checkout pinned application revision")
            });
        }
        if let Some(subdirectory) = &application.subdirectory
            && !temporary.join(subdirectory).is_dir()
        {
            remove_temporary(&temporary);
            bail!(
                "plugin {instance}: application subdirectory {subdirectory:?} does not exist at revision {}",
                application.revision
            );
        }
        if let Err(error) = self.verify_plugin_checkout(instance, application, &temporary) {
            remove_temporary(&temporary);
            return Err(error)
                .with_context(|| format!("plugin {instance}: verify cloned application source"));
        }
        let restore_empty_directory = target.exists();
        if restore_empty_directory && let Err(error) = fs::remove_dir(&target) {
            remove_temporary(&temporary);
            return Err(error)
                .with_context(|| format!("plugin {instance}: existing target is no longer empty"));
        }
        if let Err(error) = fs::rename(&temporary, &target) {
            remove_temporary(&temporary);
            if restore_empty_directory {
                let _ = fs::create_dir(&target);
            }
            return Err(error).with_context(|| {
                format!("plugin {instance}: install source at {}", target.display())
            });
        }
        if let Err(error) = write_json_atomically(&marker, application) {
            remove_temporary(&target);
            if restore_empty_directory {
                let _ = fs::create_dir(&target);
            }
            return Err(error).with_context(|| {
                format!("plugin {instance}: record source ownership; cloned source was rolled back")
            });
        }
        Ok(true)
    }

    fn verify_plugin_checkout(
        &self,
        instance: &str,
        application: &PluginApplication,
        directory: &Path,
    ) -> Result<()> {
        let head = self
            .runner
            .run("git", &["rev-parse".into(), "HEAD".into()], Some(directory))?
            .stdout
            .trim()
            .to_string();
        if head != application.revision {
            bail!(
                "plugin {instance}: source checkout is {head}, expected {}",
                application.revision
            );
        }
        let origin = self
            .runner
            .run(
                "git",
                &["remote".into(), "get-url".into(), "origin".into()],
                Some(directory),
            )?
            .stdout
            .trim()
            .to_string();
        if origin != application.repo {
            bail!(
                "plugin {instance}: source origin is {origin:?}, expected {:?}",
                application.repo
            );
        }
        let resolved = application_directory(application);
        let relative = application.subdirectory.as_deref().unwrap_or("");
        let expected = if directory == expand_path(&application.directory)? {
            resolved
        } else {
            directory.join(relative)
        };
        if !expected.is_dir() {
            bail!("plugin {instance}: resolved application directory is missing");
        }
        Ok(())
    }

    fn plugin_source_marker(&self, instance: &str) -> PathBuf {
        self.paths
            .state
            .join("plugin-sources")
            .join(format!("{instance}.json"))
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
                object.remove("env_file");
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
            checks,
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

fn remove_temporary(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("state marker has no parent")?)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}
fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}
