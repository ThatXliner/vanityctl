use std::process::Command;

use tempfile::tempdir;

const PROMPT: &str = include_str!("../prompts/install.md");

#[test]
fn onboarding_prompt_needs_no_registry_or_daemon_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vanityctl"))
        .env("VANITYCTL_CONFIG", dir.path().join("missing/config.yaml"))
        .env("CODEX_HOME", dir.path().join("codex"))
        .args(["install", "--prompt"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), PROMPT);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn onboarding_prompt_json_contains_the_canonical_text() {
    let dir = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vanityctl"))
        .env("VANITYCTL_CONFIG", dir.path().join("absent.yaml"))
        .args(["install", "--prompt", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["prompt"], PROMPT);
}

#[test]
fn prompt_mode_rejects_flags_that_change_the_machine() {
    for flag in ["--start", "--codex-skill"] {
        let output = Command::new(env!("CARGO_BIN_EXE_vanityctl"))
            .args(["install", "--prompt", flag])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}
