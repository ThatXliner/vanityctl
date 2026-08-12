use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    backend::Backend,
    config::{ConfigPaths, expand_path},
    model::{DeployTrigger, DeploymentRecord, Service},
    runner::SharedRunner,
    state::StateStore,
};

pub struct DeployCoordinator {
    runner: SharedRunner,
    state: Arc<StateStore>,
    paths: ConfigPaths,
    locks: StdMutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl DeployCoordinator {
    pub fn new(runner: SharedRunner, state: Arc<StateStore>, paths: ConfigPaths) -> Self {
        Self {
            runner,
            state,
            paths,
            locks: StdMutex::new(HashMap::new()),
        }
    }
    fn lock(&self, name: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(name.into())
            .or_default()
            .clone()
    }

    pub fn remote_commit(&self, service: &Service) -> Result<String> {
        let source = service
            .source
            .as_ref()
            .context("service has no Git source")?;
        let output = self.runner.run(
            "git",
            &[
                "ls-remote".into(),
                source.repo.clone(),
                format!("refs/heads/{}", source.branch),
            ],
            None,
        )?;
        output
            .stdout
            .split_whitespace()
            .next()
            .map(str::to_owned)
            .context("remote branch did not return a commit")
    }

    pub async fn deploy(
        &self,
        name: &str,
        service: &Service,
        backend: &dyn Backend,
        trigger: &str,
        retry: bool,
    ) -> Result<DeploymentRecord> {
        let lock = self.lock(name);
        let _guard = lock.lock().await;
        let source = service
            .source
            .as_ref()
            .context("service has no Git source")?;
        let commit = self.remote_commit(service)?;
        if !retry
            && self
                .state
                .snapshot()
                .services
                .get(name)
                .and_then(|s| s.failed_commit.as_ref())
                == Some(&commit)
        {
            bail!(
                "commit {} already failed; use --retry or push a newer commit",
                short(&commit)
            );
        }
        let id = Uuid::new_v4().to_string();
        let log_path = self
            .paths
            .logs
            .join("deployments")
            .join(name)
            .join(format!("{id}.log"));
        fs::create_dir_all(log_path.parent().unwrap())?;
        let log = Arc::new(StdMutex::new(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&log_path)?,
        ));
        write_log(
            &log,
            format!(
                "deploy {name} {}@{} ({trigger})",
                source.branch,
                short(&commit)
            ),
        )?;
        let started_at = Utc::now();
        let started = Instant::now();
        let result = self.execute(name, service, backend, &commit, &log).await;
        let status = if result.is_ok() { "success" } else { "failed" }.to_string();
        if let Err(error) = &result {
            write_log(&log, format!("ERROR: {error:#}"))?;
        }
        let record = DeploymentRecord {
            id,
            service: name.into(),
            branch: source.branch.clone(),
            commit: Some(commit.clone()),
            trigger: trigger.into(),
            started_at,
            finished_at: Utc::now(),
            duration_ms: started.elapsed().as_millis() as u64,
            status: status.clone(),
            log_file: log_path.display().to_string(),
            error: result.as_ref().err().map(|e| format!("{e:#}")),
        };
        self.state.update(|state| {
            let svc = state.services.entry(name.into()).or_default();
            svc.deployment.auto = svc
                .auto_deploy_override
                .unwrap_or_else(|| service.deploy.as_ref().map(|d| d.auto).unwrap_or(false));
            svc.deployment.branch = source.branch.clone();
            svc.deployment.remote_commit = Some(commit.clone());
            svc.deployment.last_deployment = Some(record.finished_at);
            svc.deployment.status = Some(status.clone());
            if result.is_ok() {
                svc.deployment.deployed_commit = Some(commit.clone());
                svc.failed_commit = None;
            } else {
                svc.failed_commit = Some(commit.clone());
            }
            svc.deployments.push(record.clone());
        })?;
        self.state.event(
            "deployment",
            Some(name),
            format!("deployment {status} ({})", short(&commit)),
        )?;
        result.map(|_| record)
    }

    async fn execute(
        &self,
        name: &str,
        service: &Service,
        backend: &dyn Backend,
        commit: &str,
        log: &Arc<StdMutex<fs::File>>,
    ) -> Result<()> {
        let source = service.source.as_ref().unwrap();
        let directory = expand_path(
            service
                .directory
                .as_deref()
                .context("Git-backed service requires directory")?,
        )?;
        if !directory.join(".git").exists() {
            if directory.exists() && fs::read_dir(&directory)?.next().is_some() {
                bail!(
                    "{} exists and is not an empty Git repository",
                    directory.display()
                );
            }
            if let Some(parent) = directory.parent() {
                fs::create_dir_all(parent)?;
            }
            run_logged(
                &self.runner,
                log,
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
        }
        run_logged(
            &self.runner,
            log,
            "git",
            &["fetch".into(), "origin".into(), source.branch.clone()],
            Some(&directory),
        )?;
        run_logged(
            &self.runner,
            log,
            "git",
            &["checkout".into(), source.branch.clone()],
            Some(&directory),
        )?;
        run_logged(
            &self.runner,
            log,
            "git",
            &["reset".into(), "--hard".into(), commit.into()],
            Some(&directory),
        )?;
        if let Some(deploy) = &service.deploy {
            for hook in deploy.before.iter().chain(deploy.build.iter()) {
                run_logged(
                    &self.runner,
                    log,
                    "/bin/sh",
                    &["-lc".into(), hook.clone()],
                    Some(&directory),
                )?;
            }
        }
        write_log(log, "build/hooks succeeded; reconciling workload")?;
        backend.apply(name, service).await?;
        if let Some(deploy) = &service.deploy {
            for hook in &deploy.after {
                run_logged(
                    &self.runner,
                    log,
                    "/bin/sh",
                    &["-lc".into(), hook.clone()],
                    Some(&directory),
                )?;
            }
        }
        write_log(log, "deployment succeeded")
    }

    pub fn polling_interval(service: &Service) -> Option<std::time::Duration> {
        let deploy = service.deploy.as_ref()?;
        if !deploy.auto {
            return None;
        }
        match deploy.trigger.as_ref() {
            Some(DeployTrigger::Poll { interval }) => crate::config::parse_duration(interval).ok(),
            None => Some(std::time::Duration::from_secs(60)),
            _ => None,
        }
    }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}
fn write_log(log: &Arc<StdMutex<fs::File>>, line: impl AsRef<str>) -> Result<()> {
    writeln!(
        log.lock().unwrap(),
        "[{}] {}",
        Utc::now().to_rfc3339(),
        line.as_ref()
    )?;
    Ok(())
}
fn run_logged(
    runner: &SharedRunner,
    log: &Arc<StdMutex<fs::File>>,
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<()> {
    write_log(log, format!("$ {program} {}", args.join(" ")))?;
    let output = runner.run(program, args, cwd)?;
    if !output.stdout.is_empty() {
        write!(log.lock().unwrap(), "{}", output.stdout)?;
    }
    if !output.stderr.is_empty() {
        write!(log.lock().unwrap(), "{}", output.stderr)?;
    }
    Ok(())
}
