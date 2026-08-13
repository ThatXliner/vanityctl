use std::{fs, process::Command, sync::Arc};

use tempfile::tempdir;
use vanityctl::{ConfigPaths, HostConfig, Manager, runner::RecordingRunner};

fn write_supabase_fixture(root: &std::path::Path) -> ConfigPaths {
    let paths = ConfigPaths::from_root(root);
    let project = root.join("supabase");
    fs::create_dir_all(project.join("docker")).unwrap();
    fs::write(project.join("docker/docker-compose.yml"), "services: {}\n").unwrap();
    fs::write(
        &paths.config,
        format!(
            r#"version: 1
plugins:
  supabase:
    source: stdlib:supabase-selfhost
    version: 1.0.0
services:
  data:
    type: plugin
    plugin: supabase
    directory: {}
    config:
      backup_schedule: "0 3 * * *"
    secrets:
      env_file: /private/secrets/supabase.env
"#,
            project.display()
        ),
    )
    .unwrap();
    paths
}

#[test]
fn standard_library_plugin_expands_to_normal_services() {
    let dir = tempdir().unwrap();
    let paths = write_supabase_fixture(dir.path());
    let config = HostConfig::load(&paths).unwrap();

    assert_eq!(config.services.len(), 2);
    assert_eq!(
        config.services["data"].compose_files(),
        ["docker-compose.yml"]
    );
    assert_eq!(
        config.services["data-backup"].command.as_deref(),
        Some("docker")
    );
    assert_eq!(
        config.services["data-backup"].schedule.as_deref(),
        Some("0 3 * * *")
    );
    assert_eq!(
        config.services["data"]
            .generated_by
            .as_ref()
            .unwrap()
            .plugin,
        "supabase-selfhost"
    );
    assert_eq!(
        config.resolved_plugins["data"].generated_services,
        ["data", "data-backup"]
    );
}

#[test]
fn plugin_describe_and_plan_never_expose_secret_values() {
    let dir = tempdir().unwrap();
    let paths = write_supabase_fixture(dir.path());
    let manager = Manager::new(paths, Arc::new(RecordingRunner::default())).unwrap();

    let description = serde_json::to_string(&manager.describe("data").unwrap()).unwrap();
    let plugin = serde_json::to_string(&manager.plugin("data").unwrap()).unwrap();
    let plan = serde_json::to_string(&manager.apply_plan().unwrap()).unwrap();
    for output in [&description, &plugin, &plan] {
        assert!(!output.contains("/private/secrets/supabase.env"));
    }
    assert!(description.contains("configured (value hidden)"));
    assert!(plugin.contains("env_file"));
    assert!(plan.contains("reconcile data (Compose)"));
    assert!(plan.contains("data-backup"));
}

#[test]
fn local_plugins_are_typed_pinned_and_integrity_checked() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let plugin = dir.path().join("plugins/custom");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("plugin.yaml"),
        r#"apiVersion: vanityctl.dev/plugin/v1
name: static-web
version: 2.1.0
config:
  image:
    type: string
    required: true
  enabled:
    type: boolean
    default: true
secrets:
  env_file:
    required: true
services:
  main:
    type: docker
    image: "${config.image}"
    enabled: "${config.enabled}"
    env_file: "${secrets.env_file}"
"#,
    )
    .unwrap();
    fs::write(
        &paths.config,
        r#"version: 1
plugins:
  web:
    source: path:plugins/custom
    version: 2.1.0
services:
  site:
    type: plugin
    plugin: web
    config:
      image: nginx:alpine
    secrets:
      env_file: /secret/web.env
"#,
    )
    .unwrap();

    let config = HostConfig::load(&paths).unwrap();
    assert_eq!(
        config.services["site"].image.as_deref(),
        Some("nginx:alpine")
    );
    assert!(config.services["site"].enabled);

    let integrity = config.resolved_plugins["site"].integrity.clone();
    let body = fs::read_to_string(&paths.config).unwrap();
    fs::write(
        &paths.config,
        body.replace(
            "version: 2.1.0",
            &format!("version: 2.1.0\n    integrity: {integrity}wrong"),
        ),
    )
    .unwrap();
    assert!(format!("{:#}", HostConfig::load(&paths).unwrap_err()).contains("integrity mismatch"));
}

#[test]
fn invalid_plugin_inputs_fail_before_a_runtime_command() {
    let dir = tempdir().unwrap();
    let paths = write_supabase_fixture(dir.path());
    let body = fs::read_to_string(&paths.config).unwrap();
    fs::write(
        &paths.config,
        body.replace("backup_schedule: \"0 3 * * *\"", "backup_schedule: false"),
    )
    .unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let manager = Manager::new(paths, runner.clone()).unwrap();
    assert!(manager.apply_plan().is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn git_plugins_require_and_reuse_an_exact_commit() {
    let dir = tempdir().unwrap();
    let repo = dir.path().join("plugin-repo");
    fs::create_dir_all(&repo).unwrap();
    run(&repo, &["init"]);
    run(&repo, &["config", "user.email", "test@example.com"]);
    run(&repo, &["config", "user.name", "Test"]);
    fs::write(
        repo.join("plugin.yaml"),
        "apiVersion: vanityctl.dev/plugin/v1\nname: hello\nversion: 1.0.0\nservices:\n  main:\n    type: docker\n    image: nginx:alpine\n",
    )
    .unwrap();
    run(&repo, &["add", "plugin.yaml"]);
    run(&repo, &["commit", "-m", "plugin"]);
    let sha = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let sha = sha.trim();
    let root = dir.path().join("host");
    fs::create_dir_all(&root).unwrap();
    let paths = ConfigPaths::from_root(&root);
    fs::write(
        &paths.config,
        format!(
            "version: 1\nplugins:\n  hello:\n    source: git+file://{}\n    version: 1.0.0\n    revision: {}\nservices:\n  greeting:\n    type: plugin\n    plugin: hello\n",
            repo.display(), sha
        ),
    )
    .unwrap();
    let first = HostConfig::load(&paths).unwrap();
    let second = HostConfig::load(&paths).unwrap();
    assert_eq!(
        first.services["greeting"].image.as_deref(),
        Some("nginx:alpine")
    );
    assert_eq!(
        first.resolved_plugins["greeting"].integrity,
        second.resolved_plugins["greeting"].integrity
    );
    assert!(
        paths
            .plugins
            .join("cache/hello")
            .join(sha)
            .join("plugin.yaml")
            .is_file()
    );
}

#[test]
fn git_plugins_reject_moving_references_before_cloning() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(
        &paths.config,
        "version: 1\nplugins:\n  bad:\n    source: git+https://example.invalid/plugin.git\n    version: 1.0.0\n    revision: main\nservices:\n  bad:\n    type: plugin\n    plugin: bad\n",
    )
    .unwrap();
    let error = format!("{:#}", HostConfig::load(&paths).unwrap_err());
    assert!(error.contains("40-character commit SHA"), "{error}");
    assert!(!paths.plugins.join("cache/bad").exists());
}

#[test]
fn plugin_application_source_is_materialized_once_at_its_pin() {
    let dir = tempdir().unwrap();
    let application_repo = dir.path().join("application");
    fs::create_dir_all(application_repo.join("deploy")).unwrap();
    run(&application_repo, &["init"]);
    run(
        &application_repo,
        &["config", "user.email", "test@example.com"],
    );
    run(&application_repo, &["config", "user.name", "Test"]);
    fs::write(
        application_repo.join("deploy/compose.yaml"),
        "services: {}\n",
    )
    .unwrap();
    run(&application_repo, &["add", "deploy/compose.yaml"]);
    run(&application_repo, &["commit", "-m", "application"]);
    let revision = git_output(&application_repo, &["rev-parse", "HEAD"]);

    let host = dir.path().join("host");
    let plugin = host.join("plugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("plugin.yaml"),
        format!(
            r#"apiVersion: vanityctl.dev/plugin/v1
name: packaged-compose
version: 1.0.0
application:
  repo: file://{}
  revision: {}
  subdirectory: deploy
services:
  main:
    type: compose
    directory: "${{instance.application_directory}}"
    file: compose.yaml
"#,
            application_repo.display(),
            revision
        ),
    )
    .unwrap();
    let paths = ConfigPaths::from_root(&host);
    let target = dir.path().join("installed-app");
    fs::write(
        &paths.config,
        format!(
            "version: 1\nplugins:\n  packaged:\n    source: path:plugin\n    version: 1.0.0\nservices:\n  app:\n    type: plugin\n    plugin: packaged\n    directory: {}\n",
            target.display()
        ),
    )
    .unwrap();

    let manager = Manager::system(paths.clone()).unwrap();
    assert_eq!(manager.materialize_plugin_sources().unwrap(), ["app"]);
    assert!(target.join("deploy/compose.yaml").is_file());
    assert_eq!(git_output(&target, &["rev-parse", "HEAD"]), revision);
    assert!(paths.state.join("plugin-sources/app.json").is_file());
    assert!(manager.materialize_plugin_sources().unwrap().is_empty());

    fs::write(target.join("local-data"), "preserve").unwrap();
    fs::write(&paths.config, "version: 1\nservices: {}\n").unwrap();
    HostConfig::load(&paths).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("local-data")).unwrap(),
        "preserve"
    );
}

fn run(cwd: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success());
}

fn git_output(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn removing_a_plugin_declaration_never_removes_project_data() {
    let dir = tempdir().unwrap();
    let paths = write_supabase_fixture(dir.path());
    HostConfig::load(&paths).unwrap();
    let marker = dir.path().join("supabase/important-data");
    fs::write(&marker, "keep").unwrap();
    fs::write(&paths.config, "version: 1\nservices: {}\n").unwrap();
    HostConfig::load(&paths).unwrap();
    assert_eq!(fs::read_to_string(marker).unwrap(), "keep");
}
