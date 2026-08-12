use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, bail};
use async_trait::async_trait;
use tempfile::tempdir;
use vanityctl::{
    ConfigPaths,
    backend::Backend,
    deploy::DeployCoordinator,
    model::{RuntimeState, Service, ServiceStatus},
    runner::{CommandOutput, RecordingRunner},
    state::StateStore,
};

struct MockBackend {
    fail: AtomicBool,
    calls: AtomicUsize,
    inflight: AtomicUsize,
    max_inflight: AtomicUsize,
}

impl MockBackend {
    fn new(fail: bool) -> Self {
        Self {
            fail: AtomicBool::new(fail),
            calls: AtomicUsize::new(0),
            inflight: AtomicUsize::new(0),
            max_inflight: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Backend for MockBackend {
    async fn status(&self, name: &str, service: &Service) -> Result<ServiceStatus> {
        Ok(ServiceStatus {
            name: name.into(),
            kind: service.kind.clone(),
            state: RuntimeState::Stopped,
            health: None,
            uptime_seconds: None,
            cpu_percent: None,
            memory_bytes: None,
            pid: None,
            ports: vec![],
            details: None,
            deployment: None,
            latest_job: None,
        })
    }
    async fn apply(&self, _name: &str, _service: &Service) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_inflight.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.inflight.fetch_sub(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            bail!("simulated replacement failure")
        }
        Ok(true)
    }
    async fn start(&self, _name: &str, _service: &Service) -> Result<()> {
        Ok(())
    }
    async fn stop(&self, _name: &str, _service: &Service) -> Result<()> {
        Ok(())
    }
    async fn restart(&self, _name: &str, _service: &Service) -> Result<()> {
        Ok(())
    }
    async fn logs(&self, _name: &str, _service: &Service, _lines: usize) -> Result<String> {
        Ok(String::new())
    }
}

fn fixture() -> (
    tempfile::TempDir,
    ConfigPaths,
    Service,
    Arc<RecordingRunner>,
    Arc<StateStore>,
) {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    paths.ensure_runtime_dirs().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(source.join(".git")).unwrap();
    let service: Service = serde_yaml::from_str(&format!(
        "type: docker\nimage: example:test\ndirectory: {}\nsource:\n  type: git\n  repo: git@example.invalid/repo.git\n  branch: main\ndeploy: {{ auto: true }}\n",
        source.display()
    )).unwrap();
    let runner = Arc::new(RecordingRunner::default());
    *runner.response.lock().unwrap() = Some(CommandOutput {
        stdout: format!("{}\trefs/heads/main\n", "a".repeat(40)),
        stderr: String::new(),
        code: 0,
    });
    let state = Arc::new(StateStore::load(&paths).unwrap());
    (dir, paths, service, runner, state)
}

#[tokio::test]
async fn failed_commit_is_recorded_and_not_retried_implicitly() {
    let (_dir, paths, service, runner, state) = fixture();
    let coordinator = DeployCoordinator::new(runner, state.clone(), paths);
    let backend = MockBackend::new(true);
    assert!(
        coordinator
            .deploy("web", &service, &backend, "git-poll", false)
            .await
            .is_err()
    );
    assert!(
        coordinator
            .deploy("web", &service, &backend, "git-poll", false)
            .await
            .is_err()
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let snapshot = state.snapshot();
    assert_eq!(snapshot.services["web"].deployments.len(), 1);
    assert_eq!(
        snapshot.services["web"].deployment.status.as_deref(),
        Some("failed")
    );
}

#[tokio::test]
async fn deployments_for_one_service_never_overlap() {
    let (_dir, paths, service, runner, state) = fixture();
    let coordinator = Arc::new(DeployCoordinator::new(runner, state, paths));
    let backend = Arc::new(MockBackend::new(false));
    let first = {
        let c = coordinator.clone();
        let b = backend.clone();
        let s = service.clone();
        tokio::spawn(async move { c.deploy("web", &s, b.as_ref(), "manual", false).await })
    };
    let second = {
        let c = coordinator.clone();
        let b = backend.clone();
        let s = service.clone();
        tokio::spawn(async move { c.deploy("web", &s, b.as_ref(), "manual", false).await })
    };
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert_eq!(backend.max_inflight.load(Ordering::SeqCst), 1);
}
