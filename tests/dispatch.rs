use std::{fs, sync::Arc};

use tempfile::tempdir;
use vanityctl::{
    ConfigPaths, Manager,
    runner::{CommandOutput, RecordingRunner},
};

#[tokio::test]
async fn lifecycle_dispatches_to_the_correct_backend() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(&paths.config,"version: 1\nservices:\n  web:\n    type: docker\n    image: nginx\n  worker:\n    type: process\n    command: /bin/sleep\n    args: ['5']\n").unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();
    manager.action("web", "restart").await.unwrap();
    manager.action("worker", "restart").await.unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0].0, "docker");
    assert_eq!(calls[0].1[0], "restart");
    assert_eq!(calls[1].0, "launchctl");
    assert!(calls[1].1.contains(&"kickstart".to_string()));
}

#[tokio::test]
async fn docker_status_handles_failure_as_stopped() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(
        &paths.config,
        "version: 1\nservices:\n  web:\n    type: docker\n    image: nginx\n",
    )
    .unwrap();
    let runner = Arc::new(RecordingRunner::default());
    *runner.response.lock().unwrap() = Some(CommandOutput {
        stdout: "".into(),
        stderr: "not found".into(),
        code: 1,
    });
    let manager = Manager::new(paths, runner).unwrap();
    let status = manager.status("web").await.unwrap();
    assert_eq!(format!("{:?}", status.state), "Unknown");
}
