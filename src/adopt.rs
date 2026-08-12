use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use plist::{Dictionary, Value};
use serde::Serialize;

use crate::{
    backend::render_launchd_plist,
    config::{ConfigPaths, HostConfig},
    model::{RestartPolicy, Service, ServiceType},
    runner::SharedRunner,
};

const OWNERSHIP: &str = "Adopted by vanityctl; source plist archived for rollback.";

/// A redacted, inspect-first launchd adoption result. Environment values are never retained here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptionResult {
    pub label: String,
    pub service: String,
    pub source_plist: PathBuf,
    pub archived_plist: PathBuf,
    pub service_file: PathBuf,
    pub managed_plist: PathBuf,
    pub service_type: ServiceType,
    pub command: String,
    pub arguments: Vec<String>,
    pub working_directory: Option<String>,
    pub environment_names: Vec<String>,
    pub original_stdout: Option<String>,
    pub original_stderr: Option<String>,
    pub currently_loaded: bool,
    pub current_pid: Option<u32>,
    pub action: &'static str,
}

impl AdoptionResult {
    pub fn render(&self) -> String {
        let args = if self.arguments.is_empty() {
            "—".into()
        } else {
            self.arguments.join(" ")
        };
        let env = if self.environment_names.is_empty() {
            "—".into()
        } else {
            format!("{} (values redacted)", self.environment_names.join(", "))
        };
        format!(
            "launchd adoption {}\n\nLabel:       {}\nService:     {} ({:?})\nCommand:     {}\nArguments:   {}\nDirectory:   {}\nEnvironment: {}\nLoaded:      {}{}\nOld stdout:  {}\nOld stderr:  {}\n\nPlan:\n  1. validate the candidate service and generated plist\n  2. archive {} without overwriting it\n  3. write {} and its owned launchd plist\n  4. unload {} before loading dev.vanityctl.{}\n  5. restore and reload the original automatically if bootstrap fails\n\n{}",
            self.action,
            self.label,
            self.service,
            self.service_type,
            self.command,
            args,
            self.working_directory.as_deref().unwrap_or("—"),
            env,
            self.currently_loaded,
            self.current_pid
                .map(|pid| format!(" (pid {pid})"))
                .unwrap_or_default(),
            self.original_stdout.as_deref().unwrap_or("—"),
            self.original_stderr.as_deref().unwrap_or("—"),
            self.source_plist.display(),
            self.service_file.display(),
            self.label,
            self.service,
            if self.action == "planned" {
                "No changes made. Re-run with --execute to perform this exact handoff."
            } else {
                "Adoption complete. The archived source plist is retained for manual rollback."
            }
        )
    }
}

pub struct LaunchdAdopter {
    paths: ConfigPaths,
    runner: SharedRunner,
    home: PathBuf,
    uid: u32,
}

impl LaunchdAdopter {
    pub fn discover(paths: ConfigPaths, runner: SharedRunner) -> Result<Self> {
        Ok(Self {
            paths,
            runner,
            home: dirs::home_dir().context("cannot determine home directory")?,
            uid: unsafe { libc::getuid() },
        })
    }

    /// Constructor for isolated integration tests and embedders.
    pub fn with_environment(
        paths: ConfigPaths,
        runner: SharedRunner,
        home: PathBuf,
        uid: u32,
    ) -> Self {
        Self {
            paths,
            runner,
            home,
            uid,
        }
    }

    pub fn adopt(&self, label: &str, service_name: &str, execute: bool) -> Result<AdoptionResult> {
        validate_name(service_name)?;
        if label.starts_with("dev.vanityctl.") {
            bail!("{label} is already a vanityctl-owned label");
        }
        let source = self
            .home
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"));
        if !source.is_file() {
            bail!(
                "no exact user LaunchAgent found at {}; system daemons and ambiguous searches are not adopted automatically",
                source.display()
            );
        }
        let parsed = ParsedAgent::read(&source, label)?;
        let config = HostConfig::load(&self.paths)?;
        if config.services.contains_key(service_name) {
            bail!("service {service_name:?} already exists; refusing to overwrite it");
        }
        let service_file = self.paths.services.join(format!("{service_name}.yaml"));
        if service_file.exists() {
            bail!("refusing to overwrite existing {}", service_file.display());
        }
        let archive = self
            .paths
            .root
            .join("adopted/launchd")
            .join(format!("{label}.plist"));
        if archive.exists() {
            bail!("rollback archive already exists at {}", archive.display());
        }
        let managed = self
            .paths
            .generated
            .join("launchd")
            .join(format!("dev.vanityctl.{service_name}.plist"));
        if managed.exists() {
            bail!("managed plist already exists at {}", managed.display());
        }

        // Validate the imported definition and generated launchd representation before mutation.
        let mut candidate_config = config;
        candidate_config
            .services
            .insert(service_name.to_owned(), parsed.service.clone());
        candidate_config
            .validate()
            .context("candidate service is invalid")?;
        let managed_body = render_launchd_plist(service_name, &parsed.service, &self.paths)
            .context("candidate cannot be represented by vanityctl launchd backend")?;
        let service_body = render_service_fragment(service_name, &parsed.service, label)?;
        let old_target = format!("gui/{}/{}", self.uid, label);
        let launchd_state = self
            .runner
            .run("launchctl", &["print".into(), old_target.clone()], None)
            .ok();
        let currently_loaded = launchd_state.is_some();
        let current_pid = launchd_state
            .as_ref()
            .and_then(|output| parse_pid(&output.stdout));
        let mut result = AdoptionResult {
            label: label.into(),
            service: service_name.into(),
            source_plist: source.clone(),
            archived_plist: archive.clone(),
            service_file: service_file.clone(),
            managed_plist: managed.clone(),
            service_type: parsed.service.kind.clone(),
            command: parsed.service.command.clone().unwrap_or_default(),
            arguments: parsed.service.args.clone(),
            working_directory: parsed.service.directory.clone(),
            environment_names: parsed.environment_names,
            original_stdout: parsed.original_stdout,
            original_stderr: parsed.original_stderr,
            currently_loaded,
            current_pid,
            action: "planned",
        };
        if !execute {
            return Ok(result);
        }
        if !currently_loaded {
            bail!(
                "{label} is not loaded in gui/{}; refusing an ambiguous handoff. Load it first or migrate it manually",
                self.uid
            );
        }

        fs::create_dir_all(archive.parent().unwrap())?;
        fs::create_dir_all(service_file.parent().unwrap())?;
        fs::create_dir_all(managed.parent().unwrap())?;
        let service_tmp = temporary_path(&service_file);
        let managed_tmp = temporary_path(&managed);
        fs::write(&service_tmp, service_body)?;
        fs::write(&managed_tmp, managed_body)?;

        // Moving first prevents the original from being automatically loaded again next login.
        fs::rename(&source, &archive).context("archive original plist")?;
        if let Err(error) =
            fs::rename(&service_tmp, &service_file).and_then(|_| fs::rename(&managed_tmp, &managed))
        {
            rollback_files(
                &source,
                &archive,
                &service_file,
                &managed,
                &service_tmp,
                &managed_tmp,
            );
            return Err(error).context("install candidate files; original plist restored");
        }

        if let Err(error) =
            self.runner
                .run("launchctl", &["bootout".into(), old_target.clone()], None)
        {
            rollback_files(
                &source,
                &archive,
                &service_file,
                &managed,
                &service_tmp,
                &managed_tmp,
            );
            return Err(error)
                .context("unload original; its plist was restored and remained loaded");
        }
        let new_target = format!("gui/{}/dev.vanityctl.{service_name}", self.uid);
        let bootstrap = self.runner.run(
            "launchctl",
            &[
                "bootstrap".into(),
                format!("gui/{}", self.uid),
                managed.display().to_string(),
            ],
            None,
        );
        if let Err(install_error) = bootstrap {
            // A failed bootstrap may still have partially loaded the new label.
            self.runner
                .run("launchctl", &["bootout".into(), new_target], None)
                .ok();
            rollback_files(
                &source,
                &archive,
                &service_file,
                &managed,
                &service_tmp,
                &managed_tmp,
            );
            let restore = self.runner.run(
                "launchctl",
                &[
                    "bootstrap".into(),
                    format!("gui/{}", self.uid),
                    source.display().to_string(),
                ],
                None,
            );
            return match restore {
                Ok(_) => Err(install_error).context("install managed service; original restored"),
                Err(restore_error) => bail!(
                    "install managed service failed: {install_error:#}; original files were restored but reload also failed: {restore_error:#}; run `launchctl bootstrap gui/{} {}`",
                    self.uid,
                    source.display()
                ),
            };
        }
        result.action = "completed";
        Ok(result)
    }
}

fn parse_pid(output: &str) -> Option<u32> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = ")?.parse().ok())
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.vanityctl-tmp",
        path.extension().and_then(|x| x.to_str()).unwrap_or("file")
    ))
}

fn rollback_files(
    source: &Path,
    archive: &Path,
    service: &Path,
    managed: &Path,
    service_tmp: &Path,
    managed_tmp: &Path,
) {
    fs::remove_file(service).ok();
    fs::remove_file(managed).ok();
    fs::remove_file(service_tmp).ok();
    fs::remove_file(managed_tmp).ok();
    if archive.exists() && !source.exists() {
        fs::rename(archive, source).ok();
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("service name may only contain letters, numbers, '-' and '_'");
    }
    Ok(())
}

fn render_service_fragment(name: &str, service: &Service, source_label: &str) -> Result<String> {
    #[derive(Serialize)]
    struct Fragment<'a> {
        services: BTreeMap<&'a str, &'a Service>,
    }
    let fragment = Fragment {
        services: BTreeMap::from([(name, service)]),
    };
    Ok(format!(
        "# {OWNERSHIP}\n# Original label: {source_label}\n{}",
        serde_yaml::to_string(&fragment)?
    ))
}

struct ParsedAgent {
    service: Service,
    environment_names: Vec<String>,
    original_stdout: Option<String>,
    original_stderr: Option<String>,
}

impl ParsedAgent {
    fn read(path: &Path, expected_label: &str) -> Result<Self> {
        let value = Value::from_file(path).with_context(|| format!("parse {}", path.display()))?;
        let dict = value
            .as_dictionary()
            .context("launchd plist root must be a dictionary")?;
        let label = string(dict, "Label")?.context("launchd plist has no Label")?;
        if label != expected_label {
            bail!(
                "plist Label {label:?} does not match requested label {expected_label:?}; refusing ambiguous adoption"
            );
        }
        reject_unsupported(dict)?;
        let program = string(dict, "Program")?;
        let program_arguments = string_array(dict, "ProgramArguments")?;
        if program.is_some() && program_arguments.is_some() {
            bail!(
                "Program together with ProgramArguments has argv semantics vanityctl cannot preserve safely"
            );
        }
        let (command, args) = match (program, program_arguments) {
            (Some(command), None) => (command, Vec::new()),
            (None, Some(mut values)) if !values.is_empty() => (values.remove(0), values),
            _ => bail!("exactly one of Program or non-empty ProgramArguments is required"),
        };
        let keep_alive = boolean(dict, "KeepAlive")?.unwrap_or(false);
        let run_at_load = boolean(dict, "RunAtLoad")?.unwrap_or(false);
        let interval = integer(dict, "StartInterval")?;
        let calendar = dict.get("StartCalendarInterval");
        if interval.is_some() && calendar.is_some() {
            bail!("both StartInterval and StartCalendarInterval are ambiguous");
        }
        let schedule = if let Some(seconds) = interval {
            if seconds <= 0 || seconds % 60 != 0 {
                bail!("StartInterval must be a positive whole number of minutes");
            }
            Some(format!("*/{} * * * *", seconds / 60))
        } else if let Some(calendar) = calendar {
            Some(parse_calendar(calendar)?)
        } else {
            None
        };
        let kind = if schedule.is_some() {
            if keep_alive || run_at_load {
                bail!(
                    "scheduled agents with KeepAlive or RunAtLoad cannot be represented faithfully"
                );
            }
            ServiceType::Job
        } else {
            if !run_at_load && !keep_alive {
                bail!("on-demand agent has neither a supported schedule, RunAtLoad, nor KeepAlive");
            }
            ServiceType::Process
        };
        let environment_names = environment_names(dict)?;
        if !environment_names.is_empty() {
            bail!(
                "EnvironmentVariables detected ({}) but values cannot be imported without putting secrets in declarative config; move them to env_file and adopt manually",
                environment_names.join(", ")
            );
        }
        Ok(Self {
            service: Service {
                kind,
                description: Some(format!("Adopted from launchd label {expected_label}")),
                enabled: true,
                directory: string(dict, "WorkingDirectory")?,
                command: Some(command),
                args,
                image: None,
                build: None,
                file: None,
                ports: Vec::new(),
                volumes: Vec::new(),
                environment: BTreeMap::new(),
                env_file: None,
                restart: if keep_alive {
                    RestartPolicy::Always
                } else {
                    RestartPolicy::No
                },
                schedule,
                source: None,
                deploy: None,
                expose: None,
            },
            environment_names,
            original_stdout: string(dict, "StandardOutPath")?,
            original_stderr: string(dict, "StandardErrorPath")?,
        })
    }
}

fn reject_unsupported(dict: &Dictionary) -> Result<()> {
    let supported: BTreeSet<&str> = [
        "Label",
        "Program",
        "ProgramArguments",
        "WorkingDirectory",
        "RunAtLoad",
        "KeepAlive",
        "StartInterval",
        "StartCalendarInterval",
        "EnvironmentVariables",
        "StandardOutPath",
        "StandardErrorPath",
    ]
    .into_iter()
    .collect();
    let unsupported: Vec<_> = dict
        .keys()
        .filter(|key| !supported.contains(key.as_str()))
        .cloned()
        .collect();
    if !unsupported.is_empty() {
        bail!(
            "unsupported launchd keys: {}; no changes made",
            unsupported.join(", ")
        );
    }
    Ok(())
}

fn string(dict: &Dictionary, key: &str) -> Result<Option<String>> {
    dict.get(key)
        .map(|value| {
            value
                .as_string()
                .map(str::to_owned)
                .with_context(|| format!("{key} must be a string"))
        })
        .transpose()
}

fn boolean(dict: &Dictionary, key: &str) -> Result<Option<bool>> {
    dict.get(key)
        .map(|value| {
            value
                .as_boolean()
                .with_context(|| format!("{key} must be a boolean"))
        })
        .transpose()
}

fn integer(dict: &Dictionary, key: &str) -> Result<Option<i64>> {
    dict.get(key)
        .map(|value| {
            value
                .as_signed_integer()
                .with_context(|| format!("{key} must be an integer"))
        })
        .transpose()
}

fn string_array(dict: &Dictionary, key: &str) -> Result<Option<Vec<String>>> {
    dict.get(key)
        .map(|value| {
            let array = value
                .as_array()
                .with_context(|| format!("{key} must be an array"))?;
            array
                .iter()
                .map(|item| {
                    item.as_string()
                        .map(str::to_owned)
                        .context("ProgramArguments entries must be strings")
                })
                .collect()
        })
        .transpose()
}

fn environment_names(dict: &Dictionary) -> Result<Vec<String>> {
    let Some(value) = dict.get("EnvironmentVariables") else {
        return Ok(Vec::new());
    };
    let environment = value
        .as_dictionary()
        .context("EnvironmentVariables must be a dictionary")?;
    for value in environment.values() {
        if value.as_string().is_none() {
            bail!("EnvironmentVariables values must be strings");
        }
    }
    Ok(environment.keys().cloned().collect())
}

fn parse_calendar(value: &Value) -> Result<String> {
    let calendar = value
        .as_dictionary()
        .context("StartCalendarInterval arrays are not supported; expected one daily dictionary")?;
    let unsupported: Vec<_> = calendar
        .keys()
        .filter(|key| key.as_str() != "Hour" && key.as_str() != "Minute")
        .cloned()
        .collect();
    if !unsupported.is_empty() {
        bail!(
            "StartCalendarInterval fields {} cannot be represented by the current daily schedule",
            unsupported.join(", ")
        );
    }
    let hour = integer(calendar, "Hour")?.context("StartCalendarInterval requires Hour")?;
    let minute = integer(calendar, "Minute")?.context("StartCalendarInterval requires Minute")?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        bail!("StartCalendarInterval has an invalid daily time");
    }
    Ok(format!("{minute} {hour} * * *"))
}
