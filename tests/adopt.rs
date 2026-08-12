use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Result, bail};
use tempfile::tempdir;
use vanityctl::{
    ConfigPaths, HostConfig,
    adopt::LaunchdAdopter,
    runner::{CommandOutput, CommandRunner},
};

#[derive(Default)]
struct LaunchctlMock {
    calls: Mutex<Vec<Vec<String>>>,
    fail_managed_bootstrap: bool,
}

impl CommandRunner for LaunchctlMock {
    fn run(&self, program: &str, args: &[String], _cwd: Option<&Path>) -> Result<CommandOutput> {
        assert_eq!(program, "launchctl");
        self.calls.lock().unwrap().push(args.to_vec());
        if self.fail_managed_bootstrap
            && args.first().map(String::as_str) == Some("bootstrap")
            && args
                .last()
                .is_some_and(|path| path.contains("dev.vanityctl.worker.plist"))
        {
            bail!("mock bootstrap failure");
        }
        Ok(CommandOutput {
            stdout: if args.first().map(String::as_str) == Some("print") {
                "pid = 42\n".into()
            } else {
                String::new()
            },
            stderr: String::new(),
            code: 0,
        })
    }
}

fn fixture(fail_managed_bootstrap: bool) -> (tempfile::TempDir, ConfigPaths, Arc<LaunchctlMock>) {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path().join("vanity"));
    fs::create_dir_all(&paths.root).unwrap();
    fs::write(&paths.config, "version: 1\nservices: {}\n").unwrap();
    let agents = dir.path().join("home/Library/LaunchAgents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("com.example.worker.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.example.worker</string>
<key>ProgramArguments</key><array><string>/usr/bin/python3</string><string>worker.py</string></array>
<key>WorkingDirectory</key><string>/opt/worker</string>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>/tmp/old-worker.log</string>
</dict></plist>"#,
    )
    .unwrap();
    (
        dir,
        paths,
        Arc::new(LaunchctlMock {
            calls: Mutex::new(Vec::new()),
            fail_managed_bootstrap,
        }),
    )
}

#[test]
fn dry_run_is_redacted_and_does_not_mutate() {
    let (dir, paths, runner) = fixture(false);
    let source = dir
        .path()
        .join("home/Library/LaunchAgents/com.example.worker.plist");
    let before = fs::read(&source).unwrap();
    let adopter = LaunchdAdopter::with_environment(
        paths.clone(),
        runner.clone(),
        dir.path().join("home"),
        501,
    );
    let plan = adopter
        .adopt("com.example.worker", "worker", false)
        .unwrap();
    assert_eq!(plan.action, "planned");
    assert_eq!(plan.command, "/usr/bin/python3");
    assert_eq!(fs::read(source).unwrap(), before);
    assert!(!paths.services.join("worker.yaml").exists());
    assert_eq!(runner.calls.lock().unwrap().len(), 1); // inspection only
}

#[test]
fn successful_handoff_unloads_old_before_loading_new() {
    let (dir, paths, runner) = fixture(false);
    let source = dir
        .path()
        .join("home/Library/LaunchAgents/com.example.worker.plist");
    let adopter = LaunchdAdopter::with_environment(
        paths.clone(),
        runner.clone(),
        dir.path().join("home"),
        501,
    );
    let result = adopter.adopt("com.example.worker", "worker", true).unwrap();
    assert_eq!(result.action, "completed");
    assert!(!source.exists());
    assert!(result.archived_plist.exists());
    let service = fs::read_to_string(&result.service_file).unwrap();
    assert!(service.contains("Adopted by vanityctl"));
    assert!(service.contains("command: /usr/bin/python3"));
    let imported = HostConfig::load(&paths).unwrap();
    assert_eq!(
        imported.services["worker"].command.as_deref(),
        Some("/usr/bin/python3")
    );
    let managed = fs::read_to_string(&result.managed_plist).unwrap();
    assert!(managed.contains("Owned by vanityctl"));

    let calls = runner.calls.lock().unwrap();
    let bootout = calls
        .iter()
        .position(|args| args.first().map(String::as_str) == Some("bootout"))
        .unwrap();
    let bootstrap = calls
        .iter()
        .position(|args| {
            args.first().map(String::as_str) == Some("bootstrap")
                && args
                    .last()
                    .is_some_and(|x| x.contains("dev.vanityctl.worker"))
        })
        .unwrap();
    assert!(
        bootout < bootstrap,
        "new service must never overlap old label"
    );
}

#[test]
fn daily_launchd_job_becomes_a_scheduled_job() {
    let (dir, paths, runner) = fixture(false);
    let source = dir
        .path()
        .join("home/Library/LaunchAgents/com.example.worker.plist");
    fs::write(
        &source,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.example.worker</string>
<key>Program</key><string>/usr/local/bin/backup</string>
<key>StartCalendarInterval</key><dict>
  <key>Hour</key><integer>2</integer><key>Minute</key><integer>15</integer>
</dict>
</dict></plist>"#,
    )
    .unwrap();
    let adopter = LaunchdAdopter::with_environment(paths, runner, dir.path().join("home"), 501);
    let plan = adopter
        .adopt("com.example.worker", "backup", false)
        .unwrap();
    assert_eq!(plan.service_type, vanityctl::model::ServiceType::Job);
    assert_eq!(plan.command, "/usr/local/bin/backup");
}

#[test]
fn validation_failure_leaves_original_running_and_files_untouched() {
    let (dir, paths, runner) = fixture(false);
    let source = dir
        .path()
        .join("home/Library/LaunchAgents/com.example.worker.plist");
    let mut body = fs::read_to_string(&source).unwrap();
    body = body.replace(
        "</dict></plist>",
        "<key>Sockets</key><dict/></dict></plist>",
    );
    fs::write(&source, &body).unwrap();
    let adopter = LaunchdAdopter::with_environment(
        paths.clone(),
        runner.clone(),
        dir.path().join("home"),
        501,
    );
    let error = adopter
        .adopt("com.example.worker", "worker", true)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported launchd keys: Sockets")
    );
    assert_eq!(fs::read_to_string(source).unwrap(), body);
    assert!(!paths.services.join("worker.yaml").exists());
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn failed_install_rolls_back_files_and_reloads_original() {
    let (dir, paths, runner) = fixture(true);
    let source = dir
        .path()
        .join("home/Library/LaunchAgents/com.example.worker.plist");
    let original = fs::read(&source).unwrap();
    let adopter = LaunchdAdopter::with_environment(
        paths.clone(),
        runner.clone(),
        dir.path().join("home"),
        501,
    );
    let error = adopter
        .adopt("com.example.worker", "worker", true)
        .unwrap_err();
    assert!(error.to_string().contains("original restored"));
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(!paths.services.join("worker.yaml").exists());
    assert!(
        !paths
            .generated
            .join("launchd/dev.vanityctl.worker.plist")
            .exists()
    );
    let calls = runner.calls.lock().unwrap();
    assert!(calls.iter().any(|args| {
        args.first().map(String::as_str) == Some("bootstrap")
            && args
                .last()
                .is_some_and(|path| path == source.to_str().unwrap())
    }));
}

#[test]
fn environment_values_are_never_returned_or_written() {
    let (dir, paths, runner) = fixture(false);
    let source = dir
        .path()
        .join("home/Library/LaunchAgents/com.example.worker.plist");
    let secret = "top-secret-value";
    let mut body = fs::read_to_string(&source).unwrap();
    body = body.replace(
        "</dict></plist>",
        &format!(
            "<key>EnvironmentVariables</key><dict><key>API_TOKEN</key><string>{secret}</string></dict></dict></plist>"
        ),
    );
    fs::write(&source, body).unwrap();
    let adopter =
        LaunchdAdopter::with_environment(paths.clone(), runner, dir.path().join("home"), 501);
    let error = adopter
        .adopt("com.example.worker", "worker", false)
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("API_TOKEN"));
    assert!(!message.contains(secret));
    assert!(!paths.services.join("worker.yaml").exists());
}
