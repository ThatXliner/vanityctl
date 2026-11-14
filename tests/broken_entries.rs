//! One registry entry pointing at a deleted directory must not take down
//! control of every other service.

use std::{fs, path::Path, process::Output, sync::Arc, time::Duration};

use tempfile::{TempDir, tempdir};
use vanityctl::{
    ConfigPaths, HostConfig, Manager,
    model::RuntimeState,
    runner::{CommandOutput, RecordingRunner},
};

const BROKEN: &str = "billion-supabase-trial";

/// A registry with a good Docker service and a Compose fragment whose
/// directory has been deleted.
fn registry(listen: &str) -> (TempDir, ConfigPaths) {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path().join("registry"));
    fs::create_dir_all(&paths.services).unwrap();
    fs::write(
        &paths.config,
        format!("version: 1\napi:\n  listen: {listen}\nservices:\n  good:\n    type: docker\n    image: nginx\n"),
    )
    .unwrap();
    let deleted = dir.path().join("deleted/supabase-trial");
    fs::write(
        paths.services.join(format!("{BROKEN}.yaml")),
        format!(
            "services:\n  {BROKEN}:\n    type: compose\n    directory: {}\n    files: [docker-compose.yml]\n",
            deleted.display()
        ),
    )
    .unwrap();
    (dir, paths)
}

fn broken_directory(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("deleted/supabase-trial")
}

#[test]
fn loading_sets_the_broken_entry_aside_and_keeps_the_rest() {
    let (_dir, paths) = registry("127.0.0.1:7788");
    let config = HostConfig::load(&paths).unwrap();
    assert!(config.services.contains_key("good"));
    assert!(!config.services.contains_key(BROKEN));
    let reason = &config.invalid_services[BROKEN].reason;
    assert!(reason.contains("compose directory"), "{reason}");
    assert!(reason.contains("does not exist"), "{reason}");
    assert_eq!(config.problems(), vec![reason.clone()]);
}

#[test]
fn the_entry_becomes_valid_once_its_directory_appears() {
    let (dir, paths) = registry("127.0.0.1:7788");
    let project = broken_directory(&dir);
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("docker-compose.yml"), "services: {}\n").unwrap();
    let config = HostConfig::load(&paths).unwrap();
    assert!(config.services.contains_key(BROKEN));
    assert!(config.problems().is_empty());
}

#[test]
fn configuration_errors_still_fail_and_are_reported_together() {
    let (_dir, paths) = registry("127.0.0.1:7788");
    fs::write(
        paths.services.join("more.yaml"),
        "services:\n  nightly:\n    type: job\n    command: /bin/true\n    schedule: 'every day'\n  bad name:\n    type: docker\n    image: nginx\n",
    )
    .unwrap();
    let error = format!("{:#}", HostConfig::load(&paths).unwrap_err());
    assert!(error.contains("2 problems"), "{error}");
    assert!(error.contains("invalid job schedule"), "{error}");
    assert!(error.contains("\"bad name\""), "{error}");

    fs::write(
        paths.services.join("more.yaml"),
        "services:\n  good:\n    type: docker\n    image: nginx\n",
    )
    .unwrap();
    let error = format!("{:#}", HostConfig::load(&paths).unwrap_err());
    assert!(error.contains("duplicate service \"good\""), "{error}");
}

#[tokio::test]
async fn every_operation_on_the_good_service_works_beside_a_broken_one() {
    let (_dir, paths) = registry("127.0.0.1:7788");
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();

    let list = manager.list().unwrap();
    assert_eq!(list.len(), 2);
    let broken_row = list.iter().find(|row| row["name"] == BROKEN).unwrap();
    assert_eq!(broken_row["valid"], false);
    assert!(
        broken_row["error"]
            .as_str()
            .unwrap()
            .contains("does not exist")
    );
    assert_eq!(
        list.iter().find(|row| row["name"] == "good").unwrap()["valid"],
        true
    );

    let statuses = manager.statuses().await.unwrap();
    assert_eq!(statuses.len(), 2);
    let broken_status = manager.status(BROKEN).await.unwrap();
    assert_eq!(broken_status.state, RuntimeState::Invalid);
    assert!(broken_status.details.unwrap().contains("does not exist"));
    assert_eq!(manager.status("good").await.unwrap().name, "good");

    manager.up("good").await.unwrap();
    for action in ["start", "stop", "restart"] {
        manager.action("good", action).await.unwrap();
    }
    manager.logs("good", 10).await.unwrap();

    let context = manager.agent_context_data(None).unwrap();
    assert_eq!(context["services"].as_array().unwrap().len(), 1);
    assert_eq!(context["invalidServices"][0]["name"], BROKEN);
    assert!(manager.agent_context_data(Some("good")).is_ok());
    assert!(manager.agent_context().unwrap().contains("INVALID"));

    let calls_for_good = runner.calls.lock().unwrap().len();
    for result in [
        manager.up(BROKEN).await,
        manager.action(BROKEN, "start").await,
        manager.action(BROKEN, "stop").await,
        manager.action(BROKEN, "restart").await,
        manager.logs(BROKEN, 10).await.map(|_| ()),
    ] {
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains(BROKEN), "{error}");
        assert!(error.contains("does not exist"), "{error}");
    }
    assert_eq!(
        runner.calls.lock().unwrap().len(),
        calls_for_good,
        "the broken service must never reach a backend"
    );

    let applied = manager.apply().await.unwrap();
    assert!(applied.errors[BROKEN].contains("does not exist"));
    assert!(!applied.errors.contains_key("good"));
}

fn vanityctl(config: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_vanityctl"))
        .env("VANITYCTL_CONFIG", config)
        .env_remove("VANITYCTL_API")
        .args(args)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_and_daemon_keep_working_beside_a_broken_entry() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let listen = format!("127.0.0.1:{port}");
    let (_dir, paths) = registry(&listen);
    let config = paths.config.clone();
    let runner = Arc::new(RecordingRunner::default());
    *runner.response.lock().unwrap() = Some(CommandOutput {
        stdout: String::new(),
        stderr: String::new(),
        code: 0,
    });
    let manager = Arc::new(Manager::new(paths, runner).unwrap());
    tokio::spawn(vanityctl::api::serve(manager));
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(&listen).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let run = move |args: &'static [&'static str]| {
        let config = config.clone();
        tokio::task::spawn_blocking(move || vanityctl(&config, args))
    };

    let list = run(&["list"]).await.unwrap();
    assert!(list.status.success(), "{}", text(&list));
    let list = text(&list);
    assert!(list.contains("good"), "{list}");
    assert!(list.contains("INVALID"), "{list}");
    assert!(list.contains("does not exist"), "{list}");

    let status = run(&["status"]).await.unwrap();
    assert!(status.status.success(), "{}", text(&status));
    assert!(text(&status).contains("invalid"), "{}", text(&status));

    let status = run(&["status", BROKEN, "--json"]).await.unwrap();
    assert!(status.status.success(), "{}", text(&status));
    let value: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["state"], "invalid");

    for args in [
        &["up", "good"][..],
        &["start", "good"],
        &["stop", "good"],
        &["restart", "good"],
        &["logs", "good"],
        &["agent-context"],
        &["agent-context", "good"],
    ] {
        let output = run(args).await.unwrap();
        assert!(output.status.success(), "{args:?}: {}", text(&output));
    }

    let up = run(&["up", BROKEN]).await.unwrap();
    assert!(!up.status.success());
    assert!(text(&up).contains("does not exist"), "{}", text(&up));

    let validate = run(&["config", "validate"]).await.unwrap();
    assert!(!validate.status.success());
    assert!(text(&validate).contains(BROKEN), "{}", text(&validate));

    let validate = run(&["config", "validate", "--json"]).await.unwrap();
    assert!(!validate.status.success());
    let value: serde_json::Value = serde_json::from_slice(&validate.stdout).unwrap();
    assert_eq!(value["valid"], false);
    assert_eq!(value["problems"].as_array().unwrap().len(), 1);
}
