//! Read-only host discovery. Only explicit metadata fields cross this interface.
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{HostConfig, config::expand_path, model::ServiceType, runner::CommandRunner};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
    pub schema_version: u8,
    pub resources: Vec<Resource>,
    pub warnings: Vec<String>,
    pub note: &'static str,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub kind: String,
    pub name: String,
    pub managed_as: Vec<String>,
    pub directory: Option<String>,
    pub files: Vec<String>,
    pub containers: Vec<String>,
    pub images: Vec<String>,
    pub scope: Option<String>,
    pub plist: Option<String>,
}

#[derive(Deserialize)]
struct Container {
    name: String,
    image: String,
    project: String,
    directory: String,
    files: String,
}

// Avoid docker inspect's environment, commands and arbitrary labels entirely.
const DOCKER_FORMAT: &str = r#"{"name":{{json .Names}},"image":{{json .Image}},"project":{{json (.Label "com.docker.compose.project")}},"directory":{{json (.Label "com.docker.compose.project.working_dir")}},"files":{{json (.Label "com.docker.compose.project.config_files")}}}"#;

pub fn discover(
    config: &HostConfig,
    runner: &dyn CommandRunner,
    launch_roots: &[(&Path, &str)],
    query: Option<&str>,
) -> DiscoveryReport {
    let mut resources = BTreeMap::<String, Resource>::new();
    let mut warnings = Vec::new();
    match runner.run(
        "docker",
        &[
            "ps".into(),
            "-a".into(),
            "--format".into(),
            DOCKER_FORMAT.into(),
        ],
        None,
    ) {
        Ok(output) if output.code == 0 => {
            for line in output.stdout.lines().filter(|l| !l.trim().is_empty()) {
                let Ok(c) = serde_json::from_str::<Container>(line) else {
                    warnings.push("Skipped a Docker metadata row that could not be parsed".into());
                    continue;
                };
                let key = if c.project.is_empty() {
                    format!("docker:{}", c.name)
                } else {
                    format!("compose:{}:{}", c.project, c.directory)
                };
                let resource = resources.entry(key).or_insert_with(|| {
                    let managed_as =
                        config
                            .services
                            .iter()
                            .filter(|(name, s)| {
                                if c.project.is_empty() {
                                    s.kind == ServiceType::Docker
                                        && c.name == format!("vanityctl-{name}")
                                } else {
                                    s.kind == ServiceType::Compose
                                        && s.directory
                                            .as_deref()
                                            .and_then(|d| expand_path(d).ok())
                                            .is_some_and(|d| d == Path::new(&c.directory))
                                        && (c.files.is_empty()
                                            || s.compose_files().iter().any(|f| {
                                                crate::config::resolve_compose_file(
                                                    Path::new(&c.directory),
                                                    f,
                                                )
                                                .is_ok_and(|path| {
                                                    c.files
                                                        .split(',')
                                                        .any(|observed| path == Path::new(observed))
                                                })
                                            }))
                                }
                            })
                            .map(|(name, _)| name.clone())
                            .collect();
                    Resource {
                        kind: if c.project.is_empty() {
                            "docker"
                        } else {
                            "compose"
                        }
                        .into(),
                        name: if c.project.is_empty() {
                            c.name.clone()
                        } else {
                            c.project.clone()
                        },
                        directory: (!c.directory.is_empty()).then(|| c.directory.clone()),
                        managed_as,
                        ..Resource::default()
                    }
                });
                resource.containers.push(c.name);
                resource.images.push(c.image);
                resource.files.extend(
                    c.files
                        .split(',')
                        .filter(|f| !f.is_empty())
                        .map(str::to_owned),
                );
            }
        }
        _ => {
            warnings.push("Docker discovery unavailable; check the Docker engine and retry".into())
        }
    }
    for (root, scope) in launch_roots {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                warnings.push(format!("Cannot read launchd directory {}", root.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "plist") {
                continue;
            }
            let Ok(plist) = plist::Value::from_file(&path) else {
                warnings.push(format!("Cannot read plist {}", path.display()));
                continue;
            };
            let Some(label) = plist
                .as_dictionary()
                .and_then(|p| p.get("Label"))
                .and_then(plist::Value::as_string)
            else {
                continue;
            };
            let managed_as = config
                .services
                .iter()
                .filter(|(name, s)| {
                    *scope == "user"
                        && matches!(s.kind, ServiceType::Process | ServiceType::Job)
                        && label == format!("dev.vanityctl.{name}")
                })
                .map(|(name, _)| name.clone())
                .collect();
            resources.insert(
                format!("launchd:{scope}:{}", path.display()),
                Resource {
                    kind: "launchd".into(),
                    name: label.into(),
                    managed_as,
                    scope: Some((*scope).into()),
                    plist: Some(path.display().to_string()),
                    ..Resource::default()
                },
            );
        }
    }
    let query = query.unwrap_or("").to_lowercase();
    let mut resources: Vec<_> = resources
        .into_values()
        .filter(|r| {
            r.name.to_lowercase().contains(&query)
                || r.directory
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
                || r.images
                    .iter()
                    .chain(&r.containers)
                    .chain(&r.managed_as)
                    .any(|s| s.to_lowercase().contains(&query))
                || r.managed_as.iter().any(|n| {
                    config.services[n]
                        .description
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                })
        })
        .collect();
    for resource in &mut resources {
        resource.images.sort();
        resource.images.dedup();
        let mut seen = HashSet::new();
        resource.files.retain(|file| seen.insert(file.clone()));
        resource.containers.sort();
    }
    DiscoveryReport {
        schema_version: 1,
        resources,
        warnings,
        note: "Read-only inventory, including stopped containers and installed plists. Installed does not mean running. No resources were adopted or changed.",
    }
}
