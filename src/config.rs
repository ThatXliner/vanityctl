use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::model::{DeployTrigger, DnsConfig, Service, ServiceType};

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub services: PathBuf,
    pub state: PathBuf,
    pub logs: PathBuf,
    pub generated: PathBuf,
}

impl ConfigPaths {
    pub fn discover() -> Result<Self> {
        if let Ok(path) = env::var("VANITYCTL_CONFIG") {
            let config = expand_path(&path)?;
            let root = config
                .parent()
                .context("VANITYCTL_CONFIG has no parent")?
                .to_path_buf();
            return Ok(Self::from_root_and_config(root, config));
        }
        let root = dirs::home_dir()
            .context("cannot determine home directory")?
            .join(".vanityctl");
        Ok(Self::from_root_and_config(
            root.clone(),
            root.join("config.yaml"),
        ))
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self::from_root_and_config(root.clone(), root.join("config.yaml"))
    }

    fn from_root_and_config(root: PathBuf, config: PathBuf) -> Self {
        Self {
            config,
            services: root.join("services"),
            state: root.join("state"),
            logs: root.join("logs"),
            generated: root.join("generated"),
            root,
        }
    }

    pub fn ensure_runtime_dirs(&self) -> Result<()> {
        for path in [&self.state, &self.logs, &self.generated] {
            fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub token_env: Option<String>,
}

fn default_listen() -> String {
    "127.0.0.1:7788".into()
}
impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            token_env: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub services: BTreeMap<String, Service>,
    #[serde(default)]
    pub dns: Option<DnsConfig>,
}

fn default_version() -> u32 {
    1
}

impl HostConfig {
    pub fn load(paths: &ConfigPaths) -> Result<Self> {
        let body = fs::read_to_string(&paths.config).with_context(|| {
            format!(
                "read {}; create it or set VANITYCTL_CONFIG",
                paths.config.display()
            )
        })?;
        let mut config: HostConfig = serde_yaml::from_str(&body)
            .with_context(|| format!("parse {}", paths.config.display()))?;
        if paths.services.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(&paths.services)?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("yaml" | "yml")))
                .collect();
            entries.sort();
            for path in entries {
                let body = fs::read_to_string(&path)?;
                let fragment: ServiceFragment = serde_yaml::from_str(&body)
                    .with_context(|| format!("parse {}", path.display()))?;
                for (name, service) in fragment.services {
                    if config.services.insert(name.clone(), service).is_some() {
                        bail!("duplicate service {name:?} in {}", path.display());
                    }
                }
            }
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported config version {}; expected 1", self.version);
        }
        if !self.api.listen.starts_with("127.0.0.1:")
            && !self.api.listen.starts_with("[::1]:")
            && self.api.token_env.is_none()
        {
            bail!("non-loopback api.listen requires api.token_env");
        }
        for (name, svc) in &self.services {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                bail!("service name {name:?} may only contain letters, numbers, '-' and '_'");
            }
            match svc.kind {
                ServiceType::Docker if svc.image.is_none() && svc.build.is_none() => {
                    bail!("service {name}: docker requires image or build")
                }
                ServiceType::Compose if svc.directory.is_none() => {
                    bail!("service {name}: compose requires directory")
                }
                ServiceType::Process | ServiceType::Job if svc.command.is_none() => {
                    bail!("service {name}: {:?} requires command", svc.kind)
                }
                _ => {}
            }
            if svc.kind == ServiceType::Job && svc.schedule.is_none() {
                bail!("service {name}: job requires schedule");
            }
            if let Some(schedule) = &svc.schedule {
                validate_schedule(schedule)
                    .with_context(|| format!("service {name}: invalid job schedule"))?;
            }
            if svc.kind != ServiceType::Job && svc.schedule.is_some() {
                bail!("service {name}: schedule is only valid for jobs");
            }
            if let Some(source) = &svc.source
                && source.kind != "git"
            {
                bail!("service {name}: source.type must be git");
            }
            if let Some(deploy) = &svc.deploy {
                if deploy.auto && svc.source.is_none() {
                    bail!("service {name}: deploy.auto requires source");
                }
                if deploy.auto
                    && matches!(
                        deploy.trigger,
                        Some(DeployTrigger::Webhook | DeployTrigger::Github)
                    )
                {
                    bail!(
                        "service {name}: webhook triggers are reserved for a future release; use poll"
                    );
                }
                if let Some(DeployTrigger::Poll { interval }) = &deploy.trigger {
                    parse_duration(interval)
                        .with_context(|| format!("service {name}: invalid deploy poll interval"))?;
                }
            }
            for value in svc.environment.values() {
                if value.contains("${") {
                    bail!(
                        "service {name}: environment interpolation is not supported; use env_file for secrets"
                    );
                }
            }
        }
        if let Some(dns) = &self.dns {
            if dns.provider != "cloudflare" {
                bail!(
                    "dns.provider {:?} is unsupported; V0 supports cloudflare",
                    dns.provider
                );
            }
            parse_duration(&dns.interval).context("invalid dns.interval")?;
            let mut names = HashSet::new();
            for record in &dns.records {
                if !names.insert(&record.name) {
                    bail!("duplicate DNS record {}", record.name);
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceFragment {
    services: BTreeMap<String, Service>,
}

pub fn expand_path(value: &str) -> Result<PathBuf> {
    if value == "~" {
        return dirs::home_dir().context("cannot determine home directory");
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(dirs::home_dir()
            .context("cannot determine home directory")?
            .join(rest));
    }
    Ok(Path::new(value).to_path_buf())
}

pub fn parse_duration(value: &str) -> Result<std::time::Duration> {
    let (digits, unit) = value.trim().split_at(
        value
            .trim()
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(value.len()),
    );
    let n: u64 = digits
        .parse()
        .context("duration must start with an integer")?;
    if n == 0 {
        bail!("duration must be greater than zero");
    }
    let seconds = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => bail!("duration unit must be s, m, or h"),
    };
    Ok(std::time::Duration::from_secs(seconds))
}

fn validate_schedule(cron: &str) -> Result<()> {
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
        let value: u64 = step.parse().context("minute interval must be a number")?;
        if value == 0 {
            bail!("minute interval cannot be zero");
        }
        return Ok(());
    }
    if day == "*" && month == "*" && weekday == "*" {
        let minute: u8 = minute.parse().context("minute must be a number")?;
        let hour: u8 = hour.parse().context("hour must be a number")?;
        if minute <= 59 && hour <= 23 {
            return Ok(());
        }
    }
    bail!("V0 supports exact daily time or */N minutes")
}
