use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    config::{ConfigPaths, expand_path, resolve_compose_file},
    model::{RuntimeState, Service, ServiceStatus, ServiceType},
    runner::SharedRunner,
};

#[async_trait]
pub trait Backend: Send + Sync {
    async fn status(&self, name: &str, service: &Service) -> Result<ServiceStatus>;
    async fn apply(&self, name: &str, service: &Service) -> Result<bool>;
    async fn start(&self, name: &str, service: &Service) -> Result<()>;
    async fn stop(&self, name: &str, service: &Service) -> Result<()>;
    async fn restart(&self, name: &str, service: &Service) -> Result<()>;
    async fn logs(&self, name: &str, service: &Service, lines: usize) -> Result<String>;
    async fn pull(&self, _name: &str, _service: &Service) -> Result<()> {
        bail!("pull is only supported for compose services")
    }
    async fn build(&self, _name: &str, _service: &Service) -> Result<()> {
        bail!("build is only supported for compose services")
    }
    async fn deploy(&self, name: &str, service: &Service) -> Result<()> {
        self.apply(name, service).await?;
        Ok(())
    }
}

pub struct BackendSet {
    pub docker: DockerBackend,
    pub compose: ComposeBackend,
    pub launchd: LaunchdBackend,
}

impl BackendSet {
    pub fn new(runner: SharedRunner, paths: ConfigPaths) -> Self {
        Self {
            docker: DockerBackend::new(runner.clone()),
            compose: ComposeBackend::new(runner.clone(), paths.clone()),
            launchd: LaunchdBackend::new(runner, paths),
        }
    }

    pub fn get(&self, kind: &ServiceType) -> &dyn Backend {
        match kind {
            ServiceType::Docker => &self.docker,
            ServiceType::Compose => &self.compose,
            ServiceType::Process | ServiceType::Job => &self.launchd,
            ServiceType::Plugin => {
                unreachable!("plugin declarations are resolved before backend dispatch")
            }
        }
    }
}

fn base_status(name: &str, service: &Service, state: RuntimeState) -> ServiceStatus {
    ServiceStatus {
        name: name.into(),
        kind: service.kind.clone(),
        state,
        health: None,
        uptime_seconds: None,
        cpu_percent: None,
        memory_bytes: None,
        pid: None,
        ports: service.ports.clone(),
        details: None,
        deployment: None,
        latest_job: None,
    }
}

pub struct DockerBackend {
    runner: SharedRunner,
}
impl DockerBackend {
    pub fn new(runner: SharedRunner) -> Self {
        Self { runner }
    }
}

impl DockerBackend {
    fn container(name: &str) -> String {
        format!("vanityctl-{name}")
    }
    fn hash(service: &Service) -> Result<String> {
        let bytes = serde_json::to_vec(service)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    fn build(&self, name: &str, service: &Service) -> Result<String> {
        if let Some(build) = &service.build {
            let tag = format!("vanityctl/{name}:managed");
            let cwd = service
                .directory
                .as_deref()
                .map(expand_path)
                .transpose()?
                .unwrap_or(std::env::current_dir()?);
            let context = build.context.clone().unwrap_or_else(|| ".".into());
            let mut args = vec![
                "build".into(),
                "-t".into(),
                tag.clone(),
                "-f".into(),
                build.dockerfile.clone(),
            ];
            for (key, value) in &build.args {
                args.extend(["--build-arg".into(), format!("{key}={value}")]);
            }
            args.push(context);
            self.runner.run("docker", &args, Some(&cwd))?;
            Ok(tag)
        } else {
            Ok(service.image.clone().context("docker image missing")?)
        }
    }

    fn create(&self, name: &str, service: &Service, image: String) -> Result<()> {
        let mut args = vec![
            "run".into(),
            "-d".into(),
            "--name".into(),
            Self::container(name),
            "--label".into(),
            "dev.vanityctl.managed=true".into(),
            "--label".into(),
            format!("dev.vanityctl.service={name}"),
            "--label".into(),
            format!("dev.vanityctl.config-hash={}", Self::hash(service)?),
            "--restart".into(),
            service.restart.docker_value().into(),
        ];
        for port in &service.ports {
            args.extend(["-p".into(), port.clone()]);
        }
        for volume in &service.volumes {
            args.extend(["-v".into(), expand_volume(volume)?]);
        }
        for (key, value) in &service.environment {
            args.extend(["-e".into(), format!("{key}={value}")]);
        }
        if let Some(file) = &service.env_file {
            args.extend([
                "--env-file".into(),
                expand_path(file)?.display().to_string(),
            ]);
        }
        args.push(image);
        if let Some(command) = &service.command {
            args.push(command.clone());
            args.extend(service.args.clone());
        }
        self.runner.run("docker", &args, None)?;
        Ok(())
    }
}

fn expand_volume(value: &str) -> Result<String> {
    if let Some((left, right)) = value.split_once(':') {
        Ok(format!("{}:{right}", expand_path(left)?.display()))
    } else {
        Ok(value.into())
    }
}

#[async_trait]
impl Backend for DockerBackend {
    async fn status(&self, name: &str, service: &Service) -> Result<ServiceStatus> {
        let args = vec![
            "inspect".into(),
            "--format".into(),
            "{{json .State}}".into(),
            Self::container(name),
        ];
        match self.runner.run("docker", &args, None) {
            Ok(output) => {
                let value: Value = serde_json::from_str(output.stdout.trim())
                    .context("parse docker inspect state")?;
                let running = value
                    .get("Running")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut status = base_status(
                    name,
                    service,
                    if running {
                        RuntimeState::Running
                    } else {
                        RuntimeState::Stopped
                    },
                );
                status.health = value
                    .pointer("/Health/Status")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                status.details = value
                    .get("Status")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if running
                    && let Ok(stats) = self.runner.run(
                        "docker",
                        &[
                            "stats".into(),
                            "--no-stream".into(),
                            "--format".into(),
                            "{{json .}}".into(),
                            Self::container(name),
                        ],
                        None,
                    )
                    && let Ok(v) = serde_json::from_str::<Value>(stats.stdout.trim())
                {
                    status.cpu_percent = v
                        .get("CPUPerc")
                        .and_then(Value::as_str)
                        .and_then(|x| x.trim_end_matches('%').parse().ok());
                    status.memory_bytes = v
                        .get("MemUsage")
                        .and_then(Value::as_str)
                        .and_then(|x| x.split('/').next())
                        .and_then(parse_size);
                }
                Ok(status)
            }
            Err(_) => Ok(base_status(name, service, RuntimeState::Stopped)),
        }
    }

    async fn apply(&self, name: &str, service: &Service) -> Result<bool> {
        if !service.enabled {
            self.stop(name, service).await.ok();
            return Ok(false);
        }
        let container = Self::container(name);
        let inspect = self.runner.run(
            "docker",
            &[
                "inspect".into(),
                "--format".into(),
                "{{index .Config.Labels \"dev.vanityctl.config-hash\"}}".into(),
                container.clone(),
            ],
            None,
        );
        let desired = Self::hash(service)?;
        if let Ok(output) = inspect
            && output.stdout.trim() == desired
        {
            self.runner
                .run("docker", &["start".into(), container], None)?;
            return Ok(false);
        }
        let image = self.build(name, service)?;
        self.runner
            .run("docker", &["rm".into(), "-f".into(), container], None)
            .ok();
        self.create(name, service, image)?;
        Ok(true)
    }
    async fn start(&self, name: &str, _service: &Service) -> Result<()> {
        self.runner
            .run("docker", &["start".into(), Self::container(name)], None)?;
        Ok(())
    }
    async fn stop(&self, name: &str, _service: &Service) -> Result<()> {
        self.runner
            .run("docker", &["stop".into(), Self::container(name)], None)?;
        Ok(())
    }
    async fn restart(&self, name: &str, _service: &Service) -> Result<()> {
        self.runner
            .run("docker", &["restart".into(), Self::container(name)], None)?;
        Ok(())
    }
    async fn logs(&self, name: &str, _service: &Service, lines: usize) -> Result<String> {
        Ok(self
            .runner
            .run(
                "docker",
                &[
                    "logs".into(),
                    "--tail".into(),
                    lines.to_string(),
                    Self::container(name),
                ],
                None,
            )?
            .stdout)
    }
}

pub struct ComposeBackend {
    runner: SharedRunner,
    paths: ConfigPaths,
}
impl ComposeBackend {
    pub fn new(runner: SharedRunner, paths: ConfigPaths) -> Self {
        Self { runner, paths }
    }
    fn base(service: &Service) -> Result<(Vec<String>, PathBuf)> {
        if service.file.is_some() && service.files.is_some() {
            bail!("compose accepts either legacy file or files, not both");
        }
        let files = service.compose_files();
        if files.is_empty() {
            bail!("compose requires a non-empty files list");
        }
        let cwd = expand_path(
            service
                .directory
                .as_deref()
                .context("compose directory missing")?,
        )?;
        let mut args = vec!["compose".into()];
        if let Some(file) = &service.env_file {
            args.extend([
                "--env-file".into(),
                expand_path(file)?.display().to_string(),
            ]);
        }
        let mut resolved = HashSet::new();
        for file in files {
            let path = resolve_compose_file(&cwd, file)?;
            if !resolved.insert(path.clone()) {
                bail!("compose file {} is listed more than once", path.display());
            }
            fs::File::open(&path)
                .with_context(|| format!("compose file {} is not readable", path.display()))?;
            args.extend(["-f".into(), path.display().to_string()]);
        }
        Ok((args, cwd))
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.paths
            .state
            .join("compose")
            .join(format!("{name}.sha256"))
    }

    fn fingerprint(service: &Service, cwd: &Path) -> Result<String> {
        let mut hash = Sha256::new();
        hash.update(serde_json::to_vec(service)?);
        for file in service.compose_files() {
            let path = resolve_compose_file(cwd, file)?;
            hash.update(path.to_string_lossy().as_bytes());
            hash.update(fs::read(&path).with_context(|| {
                format!("read compose file {} for reconciliation", path.display())
            })?);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    fn write_marker(&self, name: &str, fingerprint: &str) -> Result<()> {
        let marker = self.marker(name);
        fs::create_dir_all(
            marker
                .parent()
                .context("compose state path has no parent")?,
        )?;
        let temporary = marker.with_extension("sha256.tmp");
        fs::write(&temporary, fingerprint)?;
        fs::rename(temporary, marker)?;
        Ok(())
    }

    fn invalidate_marker(&self, name: &str) -> Result<()> {
        match fs::remove_file(self.marker(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn run_operation(&self, service: &Service, operation: &str) -> Result<()> {
        let (mut args, cwd) = Self::base(service)?;
        args.push(operation.into());
        self.runner.run("docker", &args, Some(&cwd))?;
        Ok(())
    }

    fn populate_metrics(&self, status: &mut ServiceStatus, container_ids: &[String]) {
        if container_ids.is_empty() {
            return;
        }
        let mut args = vec![
            "stats".into(),
            "--no-stream".into(),
            "--format".into(),
            "{{json .}}".into(),
        ];
        args.extend(container_ids.iter().cloned());
        let Ok(output) = self.runner.run("docker", &args, None) else {
            return;
        };
        (status.cpu_percent, status.memory_bytes) = parse_docker_stats(&output.stdout);
    }

    fn reconcile(&self, name: &str, service: &Service, force: bool) -> Result<bool> {
        let (mut args, cwd) = Self::base(service)?;
        let fingerprint = Self::fingerprint(service, &cwd)?;
        if !force
            && fs::read_to_string(self.marker(name)).is_ok_and(|current| current == fingerprint)
        {
            let mut status_args = args.clone();
            status_args.extend([
                "ps".into(),
                "--status".into(),
                "running".into(),
                "--quiet".into(),
            ]);
            let running = !self
                .runner
                .run("docker", &status_args, Some(&cwd))?
                .stdout
                .trim()
                .is_empty();
            if running == service.enabled {
                return Ok(false);
            }
        }
        args.extend(if service.enabled {
            vec!["up".into(), "-d".into()]
        } else {
            vec!["stop".into()]
        });
        self.runner.run("docker", &args, Some(&cwd))?;
        self.write_marker(name, &fingerprint)?;
        Ok(true)
    }
}

#[async_trait]
impl Backend for ComposeBackend {
    async fn status(&self, name: &str, service: &Service) -> Result<ServiceStatus> {
        let (mut args, cwd) = Self::base(service)?;
        args.extend([
            "ps".into(),
            "--status".into(),
            "running".into(),
            "--quiet".into(),
        ]);
        match self.runner.run("docker", &args, Some(&cwd)) {
            Ok(out) if !out.stdout.trim().is_empty() => {
                let container_ids = out
                    .stdout
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let mut status = base_status(name, service, RuntimeState::Running);
                self.populate_metrics(&mut status, &container_ids);
                Ok(status)
            }
            _ => Ok(base_status(name, service, RuntimeState::Stopped)),
        }
    }
    async fn apply(&self, name: &str, service: &Service) -> Result<bool> {
        self.reconcile(name, service, false)
    }
    async fn start(&self, _name: &str, service: &Service) -> Result<()> {
        let (mut args, cwd) = Self::base(service)?;
        args.push("start".into());
        self.runner.run("docker", &args, Some(&cwd))?;
        Ok(())
    }
    async fn stop(&self, _name: &str, service: &Service) -> Result<()> {
        let (mut args, cwd) = Self::base(service)?;
        args.push("stop".into());
        self.runner.run("docker", &args, Some(&cwd))?;
        Ok(())
    }
    async fn restart(&self, _name: &str, service: &Service) -> Result<()> {
        let (mut args, cwd) = Self::base(service)?;
        args.push("restart".into());
        self.runner.run("docker", &args, Some(&cwd))?;
        Ok(())
    }
    async fn logs(&self, _name: &str, service: &Service, lines: usize) -> Result<String> {
        let (mut args, cwd) = Self::base(service)?;
        args.extend(["logs".into(), "--tail".into(), lines.to_string()]);
        Ok(self.runner.run("docker", &args, Some(&cwd))?.stdout)
    }
    async fn pull(&self, name: &str, service: &Service) -> Result<()> {
        self.run_operation(service, "pull")?;
        self.invalidate_marker(name)
    }
    async fn build(&self, name: &str, service: &Service) -> Result<()> {
        self.run_operation(service, "build")?;
        self.invalidate_marker(name)
    }
    async fn deploy(&self, name: &str, service: &Service) -> Result<()> {
        self.run_operation(service, "pull")?;
        self.run_operation(service, "build")?;
        self.reconcile(name, service, true)?;
        Ok(())
    }
}

pub struct LaunchdBackend {
    runner: SharedRunner,
    paths: ConfigPaths,
    uid: u32,
}
impl LaunchdBackend {
    pub fn new(runner: SharedRunner, paths: ConfigPaths) -> Self {
        Self {
            runner,
            paths,
            uid: unsafe { libc::getuid() },
        }
    }
    fn label(name: &str) -> String {
        format!("dev.vanityctl.{name}")
    }
    fn plist_path(&self, name: &str) -> PathBuf {
        self.paths
            .generated
            .join("launchd")
            .join(format!("{}.plist", Self::label(name)))
    }
    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }
}

#[async_trait]
impl Backend for LaunchdBackend {
    async fn status(&self, name: &str, service: &Service) -> Result<ServiceStatus> {
        if !service.enabled {
            return Ok(base_status(name, service, RuntimeState::Disabled));
        }
        let target = format!("{}/{}", self.domain(), Self::label(name));
        match self
            .runner
            .run("launchctl", &["print".into(), target], None)
        {
            Ok(output) => {
                let mut status = base_status(
                    name,
                    service,
                    if service.kind == ServiceType::Job {
                        RuntimeState::Idle
                    } else {
                        RuntimeState::Running
                    },
                );
                status.pid = parse_launchd_pid(&output.stdout);
                if let Some(pid) = status.pid
                    && let Ok(metrics) = self.runner.run(
                        "ps",
                        &[
                            "-p".into(),
                            pid.to_string(),
                            "-o".into(),
                            "%cpu=,rss=".into(),
                        ],
                        None,
                    )
                {
                    let fields: Vec<_> = metrics.stdout.split_whitespace().collect();
                    status.cpu_percent = fields.first().and_then(|x| x.parse().ok());
                    status.memory_bytes = fields
                        .get(1)
                        .and_then(|x| x.parse::<u64>().ok())
                        .map(|kb| kb * 1024);
                }
                if service.kind == ServiceType::Process && status.pid.is_none() {
                    status.state = RuntimeState::Stopped;
                }
                Ok(status)
            }
            Err(_) => Ok(base_status(name, service, RuntimeState::Stopped)),
        }
    }
    async fn apply(&self, name: &str, service: &Service) -> Result<bool> {
        let path = self.plist_path(name);
        fs::create_dir_all(path.parent().unwrap())?;
        let desired = render_launchd_plist(name, service, &self.paths)?;
        let changed = match fs::read_to_string(&path) {
            Ok(existing) if existing == desired => false,
            Ok(existing) => {
                if !existing.contains("Owned by vanityctl") {
                    bail!("refusing to overwrite unowned file {}", path.display());
                }
                true
            }
            Err(_) => true,
        };
        if changed {
            fs::write(&path, desired).with_context(|| format!("write {}", path.display()))?;
        }
        let target = format!("{}/{}", self.domain(), Self::label(name));
        if !service.enabled {
            self.runner
                .run("launchctl", &["bootout".into(), target], None)
                .ok();
            return Ok(changed);
        }
        if changed {
            self.runner
                .run("launchctl", &["bootout".into(), target.clone()], None)
                .ok();
            self.runner.run(
                "launchctl",
                &[
                    "bootstrap".into(),
                    self.domain(),
                    path.display().to_string(),
                ],
                None,
            )?;
        }
        Ok(changed)
    }
    async fn start(&self, name: &str, _service: &Service) -> Result<()> {
        self.runner.run(
            "launchctl",
            &[
                "kickstart".into(),
                format!("{}/{}", self.domain(), Self::label(name)),
            ],
            None,
        )?;
        Ok(())
    }
    async fn stop(&self, name: &str, _service: &Service) -> Result<()> {
        self.runner.run(
            "launchctl",
            &[
                "kill".into(),
                "SIGTERM".into(),
                format!("{}/{}", self.domain(), Self::label(name)),
            ],
            None,
        )?;
        Ok(())
    }
    async fn restart(&self, name: &str, _service: &Service) -> Result<()> {
        self.runner.run(
            "launchctl",
            &[
                "kickstart".into(),
                "-k".into(),
                format!("{}/{}", self.domain(), Self::label(name)),
            ],
            None,
        )?;
        Ok(())
    }
    async fn logs(&self, name: &str, _service: &Service, lines: usize) -> Result<String> {
        tail_file(&self.paths.logs.join(format!("{name}.log")), lines)
    }
}

fn parse_launchd_pid(value: &str) -> Option<u32> {
    value
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = ")?.parse().ok())
}

pub fn render_launchd_plist(name: &str, service: &Service, paths: &ConfigPaths) -> Result<String> {
    let command = service.command.as_deref().context("command missing")?;
    let program = expand_path(command)?;
    let mut command_args = vec![program.display().to_string()];
    command_args.extend(service.args.clone());
    let args = if let Some(env_file) = &service.env_file {
        let env_file = expand_path(env_file)?;
        let mut wrapped = vec![
            "/bin/sh".into(),
            "-c".into(),
            "set -a; . \"$1\"; shift; exec \"$@\"".into(),
            "vanityctl-env".into(),
            env_file.display().to_string(),
        ];
        wrapped.extend(command_args);
        wrapped
    } else {
        command_args
    };
    let args_xml = args
        .iter()
        .map(|a| format!("    <string>{}</string>", xml_escape(a)))
        .collect::<Vec<_>>()
        .join("\n");
    let env_xml = if service.environment.is_empty() {
        String::new()
    } else {
        format!(
            "  <key>EnvironmentVariables</key>\n  <dict>\n{}\n  </dict>\n",
            service
                .environment
                .iter()
                .map(|(k, v)| format!(
                    "    <key>{}</key><string>{}</string>",
                    xml_escape(k),
                    xml_escape(v)
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let working = service
        .directory
        .as_deref()
        .map(expand_path)
        .transpose()?
        .map(|p| {
            format!(
                "  <key>WorkingDirectory</key><string>{}</string>\n",
                xml_escape(&p.display().to_string())
            )
        })
        .unwrap_or_default();
    let scheduling = match service.kind {
        ServiceType::Process => {
            let run_at_load = service.run_at_load.unwrap_or(true);
            let keep_alive = match service.restart {
                crate::model::RestartPolicy::No => {
                    "  <key>KeepAlive</key><false/>\n".to_owned()
                }
                crate::model::RestartPolicy::Always
                | crate::model::RestartPolicy::UnlessStopped => {
                    "  <key>KeepAlive</key><true/>\n".to_owned()
                }
                crate::model::RestartPolicy::OnFailure => "  <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key><false/>\n  </dict>\n".to_owned(),
            };
            format!(
                "  <key>RunAtLoad</key><{run_at_load}/>\n{keep_alive}",
                run_at_load = if run_at_load { "true" } else { "false" }
            )
        }
        ServiceType::Job => format!(
            "{}  <key>RunAtLoad</key><{}/>\n",
            render_schedule(service.schedule.as_deref().context("schedule missing")?)?,
            if service.run_at_load.unwrap_or(false) {
                "true"
            } else {
                "false"
            }
        ),
        _ => bail!("launchd only supports process and job"),
    };
    let throttle = service
        .throttle_interval
        .map(|seconds| format!("  <key>ThrottleInterval</key><integer>{seconds}</integer>\n"))
        .unwrap_or_default();
    let process_type = service
        .process_type
        .as_ref()
        .map(|kind| {
            format!(
                "  <key>ProcessType</key><string>{}</string>\n",
                kind.launchd_value()
            )
        })
        .unwrap_or_default();
    let low_priority_io = service
        .low_priority_io
        .map(|enabled| {
            format!(
                "  <key>LowPriorityIO</key><{}/>\n",
                if enabled { "true" } else { "false" }
            )
        })
        .unwrap_or_default();
    let resource_limits = service
        .resource_limits
        .as_ref()
        .and_then(|limits| limits.open_files)
        .map(|open_files| {
            format!(
                "  <key>SoftResourceLimits</key>\n  <dict>\n    <key>NumberOfFiles</key><integer>{open_files}</integer>\n  </dict>\n"
            )
        })
        .unwrap_or_default();
    let log = paths.logs.join(format!("{name}.log"));
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- Owned by vanityctl; do not edit. -->\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>dev.vanityctl.{name}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args_xml}\n  </array>\n{working}{env_xml}{scheduling}{throttle}{process_type}{low_priority_io}{resource_limits}  <key>StandardOutPath</key><string>{log}</string>\n  <key>StandardErrorPath</key><string>{log}</string>\n</dict>\n</plist>\n",
        log = xml_escape(&log.display().to_string())
    ))
}

fn render_schedule(cron: &str) -> Result<String> {
    let fields: Vec<_> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        bail!("schedule must contain five cron fields");
    }
    let (minute, hour, day, month, weekday) =
        (fields[0], fields[1], fields[2], fields[3], fields[4]);
    if let Some(step) = minute.strip_prefix("*/")
        && hour == "*"
        && day == "*"
        && month == "*"
        && weekday == "*"
    {
        let minutes: u64 = step.parse()?;
        if minutes == 0 {
            bail!("minute interval cannot be zero");
        }
        return Ok(format!(
            "  <key>StartInterval</key><integer>{}</integer>\n",
            minutes * 60
        ));
    }
    if day == "*" && month == "*" && weekday == "*" {
        let m: u8 = minute
            .parse()
            .context("V0 launchd schedules support exact daily time or */N minutes")?;
        let h: u8 = hour.parse()?;
        if m > 59 || h > 23 {
            bail!("invalid daily schedule time");
        }
        return Ok(format!(
            "  <key>StartCalendarInterval</key>\n  <dict><key>Hour</key><integer>{h}</integer><key>Minute</key><integer>{m}</integer></dict>\n"
        ));
    }
    bail!("V0 launchd schedules support exact daily time or */N minutes")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim();
    let split = value.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let n: f64 = value[..split].parse().ok()?;
    let unit = value[split..].trim();
    let multiplier = match unit {
        "B" => 1.0,
        "kB" | "KB" => 1_000.0,
        "KiB" => 1024.0,
        "MB" => 1_000_000.0,
        "MiB" => 1_048_576.0,
        "GB" => 1_000_000_000.0,
        "GiB" => 1_073_741_824.0,
        _ => return None,
    };
    Some((n * multiplier) as u64)
}
fn parse_docker_stats(output: &str) -> (Option<f64>, Option<u64>) {
    let mut cpu = None;
    let mut memory = None;
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(value) = value
            .get("CPUPerc")
            .and_then(Value::as_str)
            .and_then(|value| value.trim_end_matches('%').parse::<f64>().ok())
        {
            cpu = Some(cpu.unwrap_or(0.0) + value);
        }
        if let Some(value) = value
            .get("MemUsage")
            .and_then(Value::as_str)
            .and_then(|value| value.split('/').next())
            .and_then(parse_size)
        {
            memory = Some(memory.unwrap_or(0_u64).saturating_add(value));
        }
    }
    (cpu, memory)
}
fn tail_file(path: &Path, lines: usize) -> Result<String> {
    let body = fs::read_to_string(path).unwrap_or_default();
    Ok(body
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod metric_tests {
    use super::parse_docker_stats;

    #[test]
    fn aggregates_compose_container_stats() {
        let stats = concat!(
            r#"{"CPUPerc":"1.25%","MemUsage":"512MiB / 8GiB"}"#,
            "\n",
            r#"{"CPUPerc":"2.75%","MemUsage":"1.5GiB / 8GiB"}"#,
        );
        assert_eq!(parse_docker_stats(stats), (Some(4.0), Some(2_147_483_648)));
    }
}
