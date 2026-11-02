#![cfg(target_os = "macos")]
use std::fs;
use tempfile::tempdir;
use vanityctl::{ConfigPaths, runner::RecordingRunner};

#[test]
fn installs_private_config_valid_plist_and_skill_without_touching_workloads() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    let hostd = home.join("hostd");
    fs::write(&hostd, "fixture executable").unwrap();
    let paths = ConfigPaths::from_root(home.join("registry"));
    let skill = home.join("codex/skills/vanityctl/SKILL.md");
    let runner = RecordingRunner::default();
    let report =
        vanityctl::install::install(&paths, home, &hostd, Some(&skill), false, &runner).unwrap();
    let token = fs::read(paths.root.join("api-token")).unwrap();
    let plist = fs::read(&report.daemon_plist).unwrap();
    assert!(plist::Value::from_reader_xml(plist.as_slice()).is_ok());
    assert!(skill.is_file());
    assert!(!report.started);
    assert!(runner.calls.lock().unwrap().is_empty());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(paths.root.join("api-token"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    vanityctl::install::install(&paths, home, &hostd, Some(&skill), false, &runner).unwrap();
    assert_eq!(fs::read(paths.root.join("api-token")).unwrap(), token);
    assert_eq!(fs::read(&report.daemon_plist).unwrap(), plist);
}

#[test]
fn preserves_user_owned_skill_and_creates_no_config_on_conflict() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    let hostd = home.join("hostd");
    fs::write(&hostd, "fixture executable").unwrap();
    let skill = home.join("SKILL.md");
    fs::write(&skill, "user-owned instructions").unwrap();
    let paths = ConfigPaths::from_root(home.join("registry"));
    let result = vanityctl::install::install(
        &paths,
        home,
        &hostd,
        Some(&skill),
        false,
        &RecordingRunner::default(),
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(&skill).unwrap(),
        "user-owned instructions"
    );
    assert!(!paths.config.exists());
}
