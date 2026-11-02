use std::{fs, sync::Arc};

use tempfile::tempdir;
use vanityctl::{ConfigPaths, Manager, runner::RecordingRunner};

#[tokio::test]
async fn targeted_status_does_not_probe_unrelated_services() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(&paths.config, "version: 1\nservices:\n  requested:\n    type: docker\n    image: nginx\n  unrelated:\n    type: process\n    command: /bin/sleep\n").unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();
    assert_eq!(manager.status("requested").await.unwrap().name, "requested");
    let calls = runner.calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        1,
        "single-service status must only inspect its own backend"
    );
    assert!(calls[0].1.contains(&"vanityctl-requested".into()));
}

#[tokio::test]
async fn unknown_status_runs_no_backend_commands() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(
        &paths.config,
        "version: 1\nservices:\n  app:\n    type: docker\n    image: nginx\n",
    )
    .unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();
    assert!(manager.status("missing").await.is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn up_targets_one_stack_and_reloads_external_environment_changes() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path().join("registry"));
    paths.ensure_runtime_dirs().unwrap();
    for name in ["app", "unrelated"] {
        let project = dir.path().join(name);
        fs::create_dir(&project).unwrap();
        fs::write(project.join("compose.yaml"), "services: {}\n").unwrap();
    }
    let app = dir.path().join("app");
    fs::write(&paths.config, format!("version: 1\nservices:\n  app:\n    type: compose\n    directory: {}\n    files: [compose.yaml]\n  unrelated:\n    type: compose\n    directory: {}\n    files: [compose.yaml]\n", app.display(),dir.path().join("unrelated").display())).unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();
    manager.up("app").await.unwrap();
    fs::write(app.join(".env"), "MODE=changed\n").unwrap();
    manager.up("app").await.unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    for (program, args) in calls.iter() {
        assert_eq!(program, "docker");
        assert!(args.contains(&app.join("compose.yaml").display().to_string()));
        assert!(args.ends_with(&["up".into(), "-d".into()]));
    }
}

#[test]
fn focused_context_omits_secrets_and_unrelated_service_details() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(&paths.config, "version: 1\nservices:\n  app:\n    type: process\n    command: /bin/echo\n    args: [private-fixture-value]\n    environment: {EXAMPLE_KEY: private-fixture-value}\n  other:\n    type: docker\n    image: unused-fixture-image\n").unwrap();
    let manager = Manager::new(paths, Arc::new(RecordingRunner::default())).unwrap();
    let data = manager.agent_context_data(Some("app")).unwrap();
    let serialized = data.to_string();
    assert!(!serialized.contains("private-fixture-value"));
    assert!(!serialized.contains("unused-fixture-image"));
    assert_eq!(data["services"].as_array().unwrap().len(), 1);
    assert_eq!(data["services"][0]["environmentKeys"][0], "EXAMPLE_KEY");
    assert_eq!(
        data["services"][0]["commands"]["up"],
        serde_json::json!(["vanityctl", "up", "app"])
    );
    assert!(manager.agent_context_data(Some("missing")).is_err());
}

#[test]
fn discovery_exports_metadata_only_and_never_mutates_resources() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path().join("registry"));
    paths.ensure_runtime_dirs().unwrap();
    let project = dir.path().join("app");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("compose.yaml"), "services: {}\n").unwrap();
    fs::write(&paths.config, format!("version: 1\nservices:\n  registered:\n    type: compose\n    directory: {}\n    files: [compose.yaml]\n", project.display())).unwrap();
    let config = vanityctl::HostConfig::load(&paths).unwrap();
    let runner = RecordingRunner::default();
    *runner.response.lock().unwrap() = Some(vanityctl::runner::CommandOutput {
        stdout: serde_json::json!({"name":"app-web","project":"project","image":"example/image","directory":project,"files":project.join("compose.yaml"),"Env":["EXAMPLE=private-fixture-value"]}).to_string(),
        stderr: String::new(), code: 0,
    });
    let launch = dir.path().join("agents");
    fs::create_dir(&launch).unwrap();
    let value: plist::Value = serde_json::from_value(serde_json::json!({"Label":"org.example.worker", "ProgramArguments":["/bin/echo","private-fixture-value"],"EnvironmentVariables":{"EXAMPLE":"private-fixture-value"}})).unwrap();
    value.to_file_xml(launch.join("worker.plist")).unwrap();
    let report = vanityctl::discovery::discover(&config, &runner, &[(&launch, "user")], None);
    assert_eq!(report.resources.len(), 2);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("private-fixture-value")
    );
    let compose = report
        .resources
        .iter()
        .find(|r| r.kind == "compose")
        .unwrap();
    assert_eq!(compose.managed_as, ["registered"]);
    let filtered =
        vanityctl::discovery::discover(&config, &runner, &[(&launch, "user")], Some("worker"));
    assert_eq!(filtered.resources.len(), 1);
    for (program, args) in runner.calls.lock().unwrap().iter() {
        assert_eq!(program, "docker");
        assert_eq!(args[0], "ps");
        assert!(
            !args
                .iter()
                .any(|a| a.contains(".Env") || a.contains(".Cmd"))
        );
    }
}

#[test]
fn config_validate_json_is_parseable_without_a_daemon() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(&config, "version: 1\nservices: {}\n").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_vanityctl"))
        .env("VANITYCTL_CONFIG", config)
        .args(["config", "validate", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["valid"], true);
}
