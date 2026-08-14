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

#[test]
fn preserves_advanced_launchd_process_semantics() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let process = service(
        r#"type: process
command: /usr/local/bin/worker
restart: on-failure
run_at_load: false
throttle_interval: 30
process_type: background
low_priority_io: true
resource_limits:
  open_files: 8192
"#,
    );
    let plist = render_launchd_plist("worker", &process, &paths).unwrap();
    assert!(plist.contains("<key>RunAtLoad</key><false/>"));
    assert!(plist.contains("<key>SuccessfulExit</key><false/>"));
    assert!(plist.contains("<key>ThrottleInterval</key><integer>30</integer>"));
    assert!(plist.contains("<key>ProcessType</key><string>Background</string>"));
    assert!(plist.contains("<key>LowPriorityIO</key><true/>"));
    assert!(plist.contains("<key>NumberOfFiles</key><integer>8192</integer>"));
}

#[test]
fn jobs_can_run_at_load_and_then_follow_their_schedule() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let job =
        service("type: job\ncommand: /bin/true\nschedule: '*/15 * * * *'\nrun_at_load: true\n");
    let plist = render_launchd_plist("ddns", &job, &paths).unwrap();
    assert!(plist.contains("<key>RunAtLoad</key><true/>"));
    assert!(plist.contains("<key>StartInterval</key><integer>900</integer>"));
}

#[test]
fn native_env_files_are_loaded_without_copying_secrets_into_plists() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let env_file = dir.path().join("worker.env");
    fs::write(&env_file, "API_TOKEN=top-secret\n").unwrap();
    fs::set_permissions(&env_file, fs::Permissions::from_mode(0o600)).unwrap();
    let process = service(&format!(
        "type: process\ncommand: /usr/local/bin/worker\nenv_file: {}\n",
        env_file.display()
    ));
    let plist = render_launchd_plist("worker", &process, &paths).unwrap();
    assert!(plist.contains("set -a; . &quot;$1&quot;; shift; exec &quot;$@&quot;"));
    assert!(plist.contains(&env_file.display().to_string()));
    assert!(!plist.contains("top-secret"));
}
