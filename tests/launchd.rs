use std::{fs, sync::Arc};

use tempfile::tempdir;
use vanityctl::{
    ConfigPaths, Manager, backend::render_launchd_plist, model::Service, runner::RecordingRunner,
};

fn service(yaml: &str) -> Service {
    serde_yaml::from_str(yaml).unwrap()
}

#[test]
fn generates_owned_process_and_job_plists() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let process = service(
        "type: process\ncommand: /usr/bin/python3\nargs: [server.py]\ndirectory: /tmp/app\nrestart: always\n",
    );
    let plist = render_launchd_plist("api", &process, &paths).unwrap();
    assert!(plist.contains("Owned by vanityctl"));
    assert!(plist.contains("dev.vanityctl.api"));
    assert!(plist.contains("<key>KeepAlive</key><true"));
    let job = service("type: job\ncommand: /bin/echo\nschedule: '*/30 * * * *'\n");
    let plist = render_launchd_plist("job", &job, &paths).unwrap();
    assert!(plist.contains("<key>StartInterval</key><integer>1800</integer>"));
}

#[tokio::test]
async fn launchd_apply_is_idempotent() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(&paths.config, "version: 1\nservices:\n  worker:\n    type: process\n    command: /bin/sleep\n    args: ['30']\n    restart: always\n").unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();
    let first = manager.apply().await.unwrap();
    let calls_after_first = runner.calls.lock().unwrap().len();
    assert_eq!(first.changed, vec!["worker"]);
    assert!(calls_after_first > 0);
    let second = manager.apply().await.unwrap();
    assert_eq!(second.unchanged, vec!["worker"]);
    assert_eq!(runner.calls.lock().unwrap().len(), calls_after_first);
}

#[test]
fn rejects_unsupported_cron_without_writing_resources() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let job = service("type: job\ncommand: /bin/true\nschedule: '0 4 * * 1'\n");
    assert!(
        render_launchd_plist("weekly", &job, &paths)
            .unwrap_err()
            .to_string()
            .contains("V0 launchd schedules")
    );
}
