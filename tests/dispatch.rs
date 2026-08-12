use std::{fs, sync::Arc};

use tempfile::tempdir;
use vanityctl::{
    ConfigPaths, Manager,
    backend::{Backend, ComposeBackend},
    model::Service,
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

fn compose_fixture() -> (
    tempfile::TempDir,
    ConfigPaths,
    Service,
    Arc<RecordingRunner>,
) {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path().join("config"));
    paths.ensure_runtime_dirs().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("compose.yaml"), "services: {}\n").unwrap();
    fs::write(project.join("compose.production.yaml"), "services: {}\n").unwrap();
    let service = serde_yaml::from_str(&format!(
        "type: compose\ndirectory: {}\nfiles: [compose.yaml, compose.production.yaml]\n",
        project.display()
    ))
    .unwrap();
    (dir, paths, service, Arc::new(RecordingRunner::default()))
}

#[tokio::test]
async fn compose_commands_preserve_all_files_in_order() {
    let (_dir, paths, service, runner) = compose_fixture();
    let backend = ComposeBackend::new(runner.clone(), paths);

    backend.status("app", &service).await.unwrap();
    backend.start("app", &service).await.unwrap();
    backend.stop("app", &service).await.unwrap();
    backend.restart("app", &service).await.unwrap();
    backend.logs("app", &service, 25).await.unwrap();
    backend.pull("app", &service).await.unwrap();
    backend.build("app", &service).await.unwrap();
    backend.deploy("app", &service).await.unwrap();

    let project = std::path::Path::new(service.directory.as_deref().unwrap());
    let prefix = vec![
        "compose".to_string(),
        "-f".to_string(),
        project.join("compose.yaml").display().to_string(),
        "-f".to_string(),
        project
            .join("compose.production.yaml")
            .display()
            .to_string(),
    ];
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 10);
    for (_, args) in calls.iter() {
        assert_eq!(&args[..prefix.len()], prefix.as_slice());
    }
    assert_eq!(
        calls[0].1[prefix.len()..],
        ["ps", "--status", "running", "--quiet"]
    );
    assert_eq!(calls[1].1[prefix.len()..], ["start"]);
    assert_eq!(calls[2].1[prefix.len()..], ["stop"]);
    assert_eq!(calls[3].1[prefix.len()..], ["restart"]);
    assert_eq!(calls[4].1[prefix.len()..], ["logs", "--tail", "25"]);
    assert_eq!(calls[5].1[prefix.len()..], ["pull"]);
    assert_eq!(calls[6].1[prefix.len()..], ["build"]);
    assert_eq!(calls[7].1[prefix.len()..], ["pull"]);
    assert_eq!(calls[8].1[prefix.len()..], ["build"]);
    assert_eq!(calls[9].1[prefix.len()..], ["up", "-d"]);
}

#[tokio::test]
async fn compose_apply_is_idempotent_and_fingerprints_every_file() {
    let (_dir, paths, service, runner) = compose_fixture();
    let backend = ComposeBackend::new(runner.clone(), paths);
    *runner.response.lock().unwrap() = Some(CommandOutput {
        stdout: "container-id\n".into(),
        stderr: String::new(),
        code: 0,
    });

    assert!(backend.apply("app", &service).await.unwrap());
    assert!(!backend.apply("app", &service).await.unwrap());
    assert_eq!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, args)| args.ends_with(&["up".into(), "-d".into()]))
            .count(),
        1
    );

    let override_file =
        std::path::Path::new(service.directory.as_deref().unwrap()).join("compose.production.yaml");
    fs::write(
        override_file,
        "services:\n  web:\n    environment: {MODE: production}\n",
    )
    .unwrap();
    assert!(backend.apply("app", &service).await.unwrap());
    assert_eq!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, args)| args.ends_with(&["up".into(), "-d".into()]))
            .count(),
        2
    );
}

#[test]
fn describe_normalizes_legacy_compose_file_to_effective_list() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path().join("config"));
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("compose.yaml"), "services: {}\n").unwrap();
    fs::create_dir_all(&paths.root).unwrap();
    fs::write(
        &paths.config,
        format!(
            "version: 1\nservices:\n  app:\n    type: compose\n    directory: {}\n    file: compose.yaml\n",
            project.display()
        ),
    )
    .unwrap();
    let manager = Manager::new(paths, Arc::new(RecordingRunner::default())).unwrap();

    let description = manager.describe("app").unwrap();
    assert_eq!(description["files"], serde_json::json!(["compose.yaml"]));
    assert_eq!(
        description["resolvedFiles"],
        serde_json::json!([project.join("compose.yaml")])
    );
    assert!(description.get("file").is_none());
}
