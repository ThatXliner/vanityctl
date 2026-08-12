use std::{collections::BTreeMap, env, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    model::{DnsConfig, DnsRecordConfig, DnsRecordType},
    state::StateStore,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordStatus {
    pub name: String,
    pub kind: String,
    pub current: Option<String>,
    pub desired: String,
    pub synced: bool,
    pub proxied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsStatus {
    pub provider: String,
    pub public_ip: String,
    pub last_check: Option<chrono::DateTime<Utc>>,
    pub last_change: Option<chrono::DateTime<Utc>>,
    pub error: Option<String>,
    pub records: Vec<DnsRecordStatus>,
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    async fn records(&self) -> Result<BTreeMap<String, (String, String)>>;
    async fn upsert(&self, record: &DnsRecordConfig, value: &str) -> Result<()>;
}

pub struct CloudflareProvider {
    client: Client,
    zone_id: String,
    token: String,
}
impl CloudflareProvider {
    pub fn from_config(config: &DnsConfig) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            zone_id: config.zone_id.clone(),
            token: env::var(&config.token_env).with_context(|| {
                format!(
                    "DNS credential environment variable {} is not set",
                    config.token_env
                )
            })?,
        })
    }
}

#[derive(Deserialize)]
struct CfResponse<T> {
    success: bool,
    result: T,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct CfRecord {
    id: String,
    name: String,
    content: String,
    #[serde(rename = "type")]
    kind: String,
}

#[async_trait]
impl DnsProvider for CloudflareProvider {
    async fn records(&self) -> Result<BTreeMap<String, (String, String)>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records?per_page=500",
            self.zone_id
        );
        let response: CfResponse<Vec<CfRecord>> = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if !response.success {
            bail!("Cloudflare API error: {:?}", response.errors);
        }
        Ok(response
            .result
            .into_iter()
            .map(|r| (format!("{}:{}", r.kind, r.name), (r.id, r.content)))
            .collect())
    }
    async fn upsert(&self, record: &DnsRecordConfig, value: &str) -> Result<()> {
        let records = self.records().await?;
        let kind = dns_kind(&record.kind);
        let key = format!("{kind}:{}", record.name);
        let body = json!({"type":kind,"name":record.name,"content":value,"proxied":record.proxied});
        let request = if let Some((id, _)) = records.get(&key) {
            self.client.put(format!(
                "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{id}",
                self.zone_id
            ))
        } else {
            self.client.post(format!(
                "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
                self.zone_id
            ))
        };
        let response: CfResponse<serde_json::Value> = request
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if !response.success {
            bail!("Cloudflare API error: {:?}", response.errors);
        }
        Ok(())
    }
}

pub struct DnsReconciler {
    client: Client,
    state: Arc<StateStore>,
}
impl DnsReconciler {
    pub fn new(state: Arc<StateStore>) -> Self {
        Self {
            client: Client::new(),
            state,
        }
    }
    pub async fn public_ip(&self) -> Result<String> {
        Ok(self
            .client
            .get("https://api.ipify.org")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?
            .trim()
            .to_owned())
    }
    pub async fn status(&self, config: &DnsConfig) -> Result<DnsStatus> {
        let provider = CloudflareProvider::from_config(config)?;
        self.status_with(config, &provider).await
    }
    pub async fn reconcile(&self, config: &DnsConfig) -> Result<DnsStatus> {
        let provider = CloudflareProvider::from_config(config)?;
        self.reconcile_with(config, &provider).await
    }
    async fn status_with(
        &self,
        config: &DnsConfig,
        provider: &dyn DnsProvider,
    ) -> Result<DnsStatus> {
        let public_ip = self.public_ip().await?;
        let existing = provider.records().await?;
        let snapshot = self.state.snapshot();
        let records = config
            .records
            .iter()
            .map(|record| {
                let desired = desired_value(record, &public_ip);
                let current = existing
                    .get(&format!("{}:{}", dns_kind(&record.kind), record.name))
                    .map(|(_, v)| v.clone());
                DnsRecordStatus {
                    name: record.name.clone(),
                    kind: dns_kind(&record.kind).into(),
                    synced: current.as_deref() == Some(&desired),
                    current,
                    desired,
                    proxied: record.proxied,
                }
            })
            .collect();
        Ok(DnsStatus {
            provider: config.provider.clone(),
            public_ip,
            last_check: snapshot.dns_last_check,
            last_change: snapshot.dns_last_change,
            error: snapshot.dns_error,
            records,
        })
    }
    async fn reconcile_with(
        &self,
        config: &DnsConfig,
        provider: &dyn DnsProvider,
    ) -> Result<DnsStatus> {
        let before = self.status_with(config, provider).await?;
        let mut changed = false;
        for (record, status) in config.records.iter().zip(&before.records) {
            if !status.synced {
                provider.upsert(record, &status.desired).await?;
                changed = true;
            }
        }
        self.state.update(|state| {
            state.public_ip = Some(before.public_ip.clone());
            state.dns_last_check = Some(Utc::now());
            state.dns_error = None;
            if changed {
                state.dns_last_change = Some(Utc::now());
            }
        })?;
        self.state.event(
            "dns",
            None,
            if changed {
                "DNS records reconciled"
            } else {
                "DNS records already synchronized"
            },
        )?;
        self.status_with(config, provider).await
    }
}

fn desired_value(record: &DnsRecordConfig, public_ip: &str) -> String {
    if record.value == "public_ip" {
        public_ip.into()
    } else {
        record.value.clone()
    }
}
fn dns_kind(kind: &DnsRecordType) -> &'static str {
    match kind {
        DnsRecordType::A => "A",
        DnsRecordType::Aaaa => "AAAA",
        DnsRecordType::Cname => "CNAME",
    }
}
