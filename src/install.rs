//! Local bootstrap is intentionally separate from workload reconciliation.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::json;

use crate::{ConfigPaths, HostConfig, runner::CommandRunner};

const SKILL: &str = include_str!("../skills/vanityctl/SKILL.md");
const SKILL_MARKER: &str =
    "<!-- Managed by vanityctl install; edit the canonical skill in the vanityctl repo. -->";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub config: PathBuf,
    pub daemon_plist: PathBuf,
    pub codex_skill: Option<PathBuf>,
    pub started: bool,
    pub note: &'static str,
}

fn create_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)?;
    Ok(())
}

pub fn install(
    paths: &ConfigPaths,
    home: &Path,
    hostd: &Path,
    skill: Option<&Path>,
    start: bool,
    runner: &dyn CommandRunner,
) -> Result<InstallReport> {
    if !cfg!(target_os = "macos") {
        bail!(
            "daemon installation currently supports macOS launchd; run hostd manually on other platforms"
        );
    }
    if !hostd.is_file() {
        bail!(
            "hostd is missing next to vanityctl; install both binaries with `cargo install --locked --path .`"
        );
    }
    // Preflight user-owned artifacts before changing anything.
    if let Some(skill) = skill
        && skill.exists()
        && !fs::read_to_string(skill)?.contains(SKILL_MARKER)
    {
        bail!(
            "{} already contains a user-owned skill; preserve or relocate it before installing",
            skill.display()
        );
    }
    let target = home.join("Library/LaunchAgents/dev.vanityctl.hostd.plist");
    let expected = vec![
        hostd.display().to_string(),
        "--config".into(),
        paths.config.display().to_string(),
    ];
    if target.exists() {
        let existing = plist::Value::from_file(&target)?;
        let arguments: Vec<_> = existing
            .as_dictionary()
            .and_then(|p| p.get("ProgramArguments"))
            .and_then(plist::Value::as_array)
            .context("existing hostd plist has no ProgramArguments; inspect it manually")?
            .iter()
            .filter_map(plist::Value::as_string)
            .map(str::to_owned)
            .collect();
        let label = existing
            .as_dictionary()
            .and_then(|p| p.get("Label"))
            .and_then(plist::Value::as_string);
        if arguments != expected || label != Some("dev.vanityctl.hostd") {
            bail!(
                "existing {} uses a different executable/config; it was not overwritten. Review that agent before reinstalling",
                target.display()
            );
        }
    }
    fs::create_dir_all(&paths.root)?;
    fs::create_dir_all(&paths.services)?;
    paths.ensure_runtime_dirs()?;
    if !paths.config.exists() {
        let token = paths.root.join("api-token");
        if !token.exists() {
            create_private(
                &token,
                format!(
                    "{}{}\n",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                )
                .as_bytes(),
            )?;
        }
        let config = json!({"version":1,"api":{"listen":"127.0.0.1:7788","token_file":token,"public_read_only":false},"services":{}});
        create_private(
            &paths.config,
            serde_json::to_string_pretty(&config)?.as_bytes(),
        )?;
    }
    HostConfig::load(paths)?;
    if !target.exists() {
        let bin = hostd.parent().context("hostd has no parent directory")?;
        let mut path_entries = vec![bin.to_path_buf()];
        path_entries.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let daemon_path = std::env::join_paths(path_entries)?
            .to_string_lossy()
            .into_owned();
        let spec = json!({"Label":"dev.vanityctl.hostd","ProgramArguments":expected,"RunAtLoad":true,"KeepAlive":true,
            "EnvironmentVariables":{"PATH":daemon_path},
            "StandardOutPath":paths.logs.join("hostd.log"),"StandardErrorPath":paths.logs.join("hostd.log")});
        let value: plist::Value = serde_json::from_value(spec).context("construct hostd plist")?;
        fs::create_dir_all(target.parent().unwrap())?;
        let mut bytes = Vec::new();
        value.to_writer_xml(&mut bytes)?;
        create_private(&target, &bytes)?;
    }
    if let Some(skill) = skill {
        fs::create_dir_all(skill.parent().context("skill has no parent directory")?)?;
        fs::write(skill, SKILL)?;
    }
    if start {
        let domain = format!("gui/{}", unsafe { libc::getuid() });
        let loaded = runner
            .run(
                "launchctl",
                &["print".into(), format!("{domain}/dev.vanityctl.hostd")],
                None,
            )
            .is_ok_and(|output| output.code == 0);
        if !loaded {
            runner.run(
                "launchctl",
                &["bootstrap".into(), domain, target.display().to_string()],
                None,
            )?;
        }
    }
    Ok(InstallReport {
        config: paths.config.clone(),
        daemon_plist: target,
        codex_skill: skill.map(Path::to_path_buf),
        started: start,
        note: "Existing config/plist preserved; no managed workloads were applied or restarted. Reload an already-running hostd separately after upgrading binaries.",
    })
}
