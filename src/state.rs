use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    config::ConfigPaths,
    model::{DeploymentRecord, DeploymentSummary, Event, JobRun},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    #[serde(default)]
    pub deployment: DeploymentSummary,
    #[serde(default)]
    pub deployments: Vec<DeploymentRecord>,
    #[serde(default)]
    pub job_runs: Vec<JobRun>,
    #[serde(default)]
    pub auto_deploy_override: Option<bool>,
    #[serde(default)]
    pub enabled_override: Option<bool>,
    #[serde(default)]
    pub failed_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistentState {
    #[serde(default)]
    pub services: BTreeMap<String, ServiceState>,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub public_ip: Option<String>,
    #[serde(default)]
    pub dns_last_check: Option<chrono::DateTime<Utc>>,
    #[serde(default)]
    pub dns_last_change: Option<chrono::DateTime<Utc>>,
    #[serde(default)]
    pub dns_error: Option<String>,
}

pub struct StateStore {
    path: PathBuf,
    inner: Mutex<PersistentState>,
}

impl StateStore {
    pub fn load(paths: &ConfigPaths) -> Result<Self> {
        paths.ensure_runtime_dirs()?;
        let path = paths.state.join("state.json");
        let value = match fs::read_to_string(&path) {
            Ok(body) => {
                serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                PersistentState::default()
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            inner: Mutex::new(value),
        })
    }

    pub fn snapshot(&self) -> PersistentState {
        self.inner.lock().unwrap().clone()
    }

    pub fn update<T>(&self, operation: impl FnOnce(&mut PersistentState) -> T) -> Result<T> {
        let mut state = self.inner.lock().unwrap();
        let result = operation(&mut state);
        if state.events.len() > 500 {
            let drain = state.events.len() - 500;
            state.events.drain(0..drain);
        }
        for service in state.services.values_mut() {
            if service.deployments.len() > 100 {
                let drain = service.deployments.len() - 100;
                service.deployments.drain(0..drain);
            }
            if service.job_runs.len() > 100 {
                let drain = service.job_runs.len() - 100;
                service.job_runs.drain(0..drain);
            }
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&*state)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &self.path).with_context(|| format!("replace {}", self.path.display()))?;
        Ok(result)
    }

    pub fn event(
        &self,
        kind: &str,
        service: Option<&str>,
        message: impl Into<String>,
    ) -> Result<()> {
        self.update(|state| {
            state.events.push(Event {
                timestamp: Utc::now(),
                kind: kind.into(),
                service: service.map(str::to_owned),
                message: message.into(),
            })
        })
    }
}
