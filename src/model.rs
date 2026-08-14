use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceType {
    Docker,
    Compose,
    Process,
    Job,
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedByPlugin {
    pub instance: String,
    pub plugin: String,
    pub version: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub materializes_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Always,
    UnlessStopped,
    OnFailure,
    #[default]
    No,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProcessType {
    Standard,
    Background,
    Interactive,
    Adaptive,
}

impl ProcessType {
    pub fn launchd_value(&self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Background => "Background",
            Self::Interactive => "Interactive",
            Self::Adaptive => "Adaptive",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    #[serde(default)]
    pub open_files: Option<u64>,
}

impl RestartPolicy {
    pub fn docker_value(&self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::UnlessStopped => "unless-stopped",
            Self::OnFailure => "on-failure",
            Self::No => "no",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    #[serde(default = "default_dockerfile")]
    pub dockerfile: String,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

fn default_dockerfile() -> String {
    "Dockerfile".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSource {
    #[serde(rename = "type", default = "git_type")]
    pub kind: String,
    pub repo: String,
    #[serde(default = "default_branch")]
    pub branch: String,
}

fn git_type() -> String {
    "git".into()
}
fn default_branch() -> String {
    "main".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DeployStrategy {
    #[default]
    Pull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DeployTrigger {
    Poll {
        #[serde(default = "default_poll_interval")]
        interval: String,
    },
    Webhook,
    Github,
}

fn default_poll_interval() -> String {
    "60s".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub strategy: DeployStrategy,
    #[serde(default)]
    pub trigger: Option<DeployTrigger>,
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub build: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposeConfig {
    pub domain: String,
    #[serde(default)]
    pub dns: bool,
    #[serde(default)]
    pub proxy: bool,
    #[serde(default)]
    pub tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    #[serde(rename = "type")]
    pub kind: ServiceType,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub build: Option<BuildConfig>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<String>>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub env_file: Option<String>,
    #[serde(default)]
    pub restart: RestartPolicy,
    /// Overrides the launchd RunAtLoad default. Processes default to true and
    /// jobs default to false when omitted.
    #[serde(default)]
    pub run_at_load: Option<bool>,
    #[serde(default)]
    pub throttle_interval: Option<u64>,
    #[serde(default)]
    pub process_type: Option<ProcessType>,
    #[serde(default)]
    pub low_priority_io: Option<bool>,
    #[serde(default)]
    pub resource_limits: Option<ResourceLimits>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub source: Option<GitSource>,
    #[serde(default)]
    pub deploy: Option<DeployConfig>,
    #[serde(default)]
    pub expose: Option<ExposeConfig>,
    /// Plugin alias for `type: plugin` declarations. Plugin declarations are
    /// expanded during config loading and never reach a runtime backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, serde_yaml::Value>,
    #[serde(default, skip_serializing)]
    pub secrets: BTreeMap<String, String>,
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<GeneratedByPlugin>,
}

impl Service {
    /// Returns the effective ordered Compose file list, normalizing the legacy
    /// singular `file` key. Configuration validation guarantees that Compose
    /// services have exactly one of `file` or `files`.
    pub fn compose_files(&self) -> Vec<&str> {
        if let Some(files) = &self.files {
            files.iter().map(String::as_str).collect()
        } else {
            self.file.iter().map(String::as_str).collect()
        }
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum DnsRecordType {
    #[default]
    #[serde(rename = "A", alias = "a")]
    A,
    #[serde(rename = "AAAA", alias = "aaaa")]
    Aaaa,
    #[serde(rename = "CNAME", alias = "cname")]
    Cname,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsRecordConfig {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: DnsRecordType,
    pub value: String,
    #[serde(default)]
    pub proxied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsConfig {
    pub provider: String,
    /// Optional provider-specific zone identifier. Omit it to discover the
    /// Cloudflare zone from the configured record names.
    #[serde(default)]
    pub zone_id: Option<String>,
    /// Concise credential form: path to a private token file.
    #[serde(default)]
    pub credentials: Option<String>,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub token_file: Option<String>,
    #[serde(default = "default_dns_interval")]
    pub interval: String,
    /// Hostnames that should follow this machine's public IPv4 address.
    #[serde(default)]
    pub dynamic: Vec<String>,
    #[serde(default)]
    pub records: Vec<DnsRecordConfig>,
}

impl DnsConfig {
    pub fn effective_records(&self) -> Vec<DnsRecordConfig> {
        self.dynamic
            .iter()
            .map(|name| DnsRecordConfig {
                name: name.clone(),
                kind: DnsRecordType::A,
                value: "public_ip".into(),
                proxied: false,
            })
            .chain(self.records.iter().cloned())
            .collect()
    }
}

fn default_dns_interval() -> String {
    "5m".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeState {
    Running,
    Stopped,
    Idle,
    Disabled,
    Unknown,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ServiceType,
    pub state: RuntimeState,
    pub health: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub pid: Option<u32>,
    pub ports: Vec<String>,
    pub details: Option<String>,
    pub deployment: Option<DeploymentSummary>,
    pub latest_job: Option<JobRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentSummary {
    pub auto: bool,
    pub branch: String,
    pub deployed_commit: Option<String>,
    pub remote_commit: Option<String>,
    pub last_deployment: Option<DateTime<Utc>>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRecord {
    pub id: String,
    pub service: String,
    pub branch: String,
    pub commit: Option<String>,
    pub trigger: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub status: String,
    pub log_file: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRun {
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub log_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub timestamp: DateTime<Utc>,
    pub kind: String,
    pub service: Option<String>,
    pub message: String,
}
