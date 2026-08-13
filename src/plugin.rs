use std::{collections::BTreeMap, fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};

use crate::{
    config::{ConfigPaths, HostConfig, expand_path},
    model::{GeneratedByPlugin, Service, ServiceType},
};

const SUPABASE_SELFHOST: &str = include_str!("../plugins/stdlib/supabase-selfhost/plugin.yaml");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDefinition {
    pub source: String,
    pub version: String,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub integrity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResolution {
    pub instance: String,
    pub alias: String,
    pub name: String,
    pub source: String,
    pub version: String,
    pub revision: Option<String>,
    pub integrity: String,
    pub generated_services: Vec<String>,
    pub config_keys: Vec<String>,
    pub secret_keys: Vec<String>,
    pub description: Option<String>,
    pub upgrade_guidance: Option<String>,
    pub removal_guidance: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginManifest {
    api_version: String,
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    config: BTreeMap<String, InputSpec>,
    #[serde(default)]
    secrets: BTreeMap<String, SecretSpec>,
    services: BTreeMap<String, Value>,
    #[serde(default)]
    upgrade_guidance: Option<String>,
    #[serde(default)]
    removal_guidance: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InputType {
    String,
    Boolean,
    Integer,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputSpec {
    #[serde(rename = "type")]
    kind: InputType,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretSpec {
    #[serde(default)]
    required: bool,
}

pub fn resolve_plugins(config: &mut HostConfig, paths: &ConfigPaths) -> Result<()> {
    validate_aliases(&config.plugins)?;
    let declarations: Vec<_> = config
        .services
        .iter()
        .filter(|(_, service)| service.kind == ServiceType::Plugin)
        .map(|(name, service)| (name.clone(), service.clone()))
        .collect();

    for (instance, declaration) in declarations {
        let alias = declaration
            .plugin
            .as_deref()
            .with_context(|| format!("service {instance}: type plugin requires plugin"))?;
        let definition = config.plugins.get(alias).with_context(|| {
            format!("service {instance}: unknown plugin alias {alias:?}; define it under plugins")
        })?;
        validate_plugin_declaration(&instance, &declaration)?;
        let (body, source_label) = load_manifest(alias, definition, paths)?;
        let integrity = format!("sha256:{:x}", Sha256::digest(body.as_bytes()));
        if let Some(expected) = &definition.integrity
            && expected != &integrity
        {
            bail!("plugin {alias}: integrity mismatch: expected {expected}, resolved {integrity}");
        }
        let manifest: PluginManifest = serde_yaml::from_str(&body)
            .with_context(|| format!("plugin {alias}: parse manifest"))?;
        validate_manifest(alias, definition, &manifest)?;
        let inputs = resolve_inputs(&instance, &manifest, &declaration.config)?;
        validate_secrets(&instance, &manifest, &declaration.secrets)?;

        config.services.remove(&instance);
        let mut generated_names = Vec::new();
        let mut templates: Vec<_> = manifest.services.iter().collect();
        templates.sort_by_key(|(key, _)| if key.as_str() == "main" { 0 } else { 1 });
        for (key, template) in templates {
            validate_template_secret_locations(alias, template, &mut Vec::new())?;
            let generated_name = if key == "main" {
                instance.clone()
            } else {
                format!("{instance}-{key}")
            };
            if config.services.contains_key(&generated_name) {
                bail!(
                    "plugin service {instance}: generated service {generated_name:?} conflicts with an existing service"
                );
            }
            let context = TemplateContext {
                instance: &instance,
                directory: declaration.directory.as_deref(),
                config: &inputs,
                secrets: &declaration.secrets,
            };
            let rendered = render_value(template, &context)?;
            let mut service: Service = serde_yaml::from_value(rendered).with_context(|| {
                format!("plugin service {instance}: generated {generated_name} is invalid")
            })?;
            if service.kind == ServiceType::Plugin {
                bail!("plugin {alias}: generated services cannot have type plugin");
            }
            service.generated_by = Some(GeneratedByPlugin {
                instance: instance.clone(),
                plugin: manifest.name.clone(),
                version: manifest.version.clone(),
                source: source_label.clone(),
                revision: definition.revision.clone(),
            });
            service.enabled = declaration.enabled && service.enabled;
            config.services.insert(generated_name.clone(), service);
            generated_names.push(generated_name);
        }
        config.resolved_plugins.insert(
            instance.clone(),
            PluginResolution {
                instance,
                alias: alias.into(),
                name: manifest.name,
                source: source_label,
                version: manifest.version,
                revision: definition.revision.clone(),
                integrity,
                generated_services: generated_names,
                config_keys: inputs.keys().cloned().collect(),
                secret_keys: declaration.secrets.keys().cloned().collect(),
                description: manifest.description,
                upgrade_guidance: manifest.upgrade_guidance,
                removal_guidance: manifest.removal_guidance,
            },
        );
    }
    Ok(())
}

fn validate_aliases(definitions: &BTreeMap<String, PluginDefinition>) -> Result<()> {
    for alias in definitions.keys() {
        if !valid_name(alias) {
            bail!("plugin alias {alias:?} may only contain letters, numbers, '-' and '_'");
        }
    }
    Ok(())
}

fn validate_plugin_declaration(name: &str, service: &Service) -> Result<()> {
    if service.command.is_some()
        || service.image.is_some()
        || service.build.is_some()
        || service.file.is_some()
        || service.files.is_some()
        || !service.ports.is_empty()
        || !service.volumes.is_empty()
        || !service.environment.is_empty()
        || service.env_file.is_some()
        || service.schedule.is_some()
        || service.source.is_some()
        || service.deploy.is_some()
        || service.expose.is_some()
        || !service.args.is_empty()
    {
        bail!(
            "service {name}: type plugin only accepts description, enabled, directory, plugin, config, and secrets"
        );
    }
    Ok(())
}

fn load_manifest(
    alias: &str,
    definition: &PluginDefinition,
    paths: &ConfigPaths,
) -> Result<(String, String)> {
    if let Some(name) = definition.source.strip_prefix("stdlib:") {
        if definition.revision.is_some() {
            bail!("plugin {alias}: revision is only valid for Git sources");
        }
        let body = match name {
            "supabase-selfhost" => SUPABASE_SELFHOST,
            _ => bail!(
                "plugin {alias}: unknown standard-library plugin {name:?}; available: supabase-selfhost"
            ),
        };
        return Ok((body.into(), definition.source.clone()));
    }
    if let Some(url) = definition.source.strip_prefix("git+") {
        let revision = definition.revision.as_deref().with_context(|| {
            format!("plugin {alias}: Git sources require revision with a full commit SHA")
        })?;
        if !is_commit_pin(revision) {
            bail!(
                "plugin {alias}: Git sources require revision to be a full 40-character commit SHA"
            );
        }
        let cache = paths.plugins.join("cache").join(alias).join(revision);
        fetch_git_plugin(alias, url, revision, &cache)?;
        let body = read_manifest_file(alias, &cache)?;
        return Ok((body, format!("git+{url}")));
    }
    if definition.revision.is_some() {
        bail!("plugin {alias}: revision is only valid for Git sources");
    }
    let raw = definition
        .source
        .strip_prefix("path:")
        .unwrap_or(&definition.source);
    let mut path = expand_path(raw)?;
    if path.is_relative() {
        path = paths.root.join(path);
    }
    let body = read_manifest_file(alias, &path)?;
    Ok((body, format!("path:{}", path.display())))
}

fn read_manifest_file(alias: &str, directory: &Path) -> Result<String> {
    let path = directory.join("plugin.yaml");
    fs::read_to_string(&path).with_context(|| format!("plugin {alias}: read {}", path.display()))
}

fn fetch_git_plugin(alias: &str, url: &str, version: &str, cache: &Path) -> Result<()> {
    if cache.join("plugin.yaml").is_file() {
        return verify_git_checkout(alias, version, cache);
    }
    if cache.exists() {
        bail!(
            "plugin {alias}: cache {} exists but has no plugin.yaml; remove that cache directory and retry",
            cache.display()
        );
    }
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent)?;
    }
    run_git(
        alias,
        None,
        &[
            "clone",
            "--no-checkout",
            "--",
            url,
            &cache.display().to_string(),
        ],
    )?;
    run_git(alias, Some(cache), &["checkout", "--detach", version])?;
    verify_git_checkout(alias, version, cache)
}

fn verify_git_checkout(alias: &str, version: &str, cache: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cache)
        .output()
        .with_context(|| format!("plugin {alias}: run git rev-parse"))?;
    if !output.status.success() {
        bail!("plugin {alias}: cached Git checkout is invalid");
    }
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if actual != version {
        bail!("plugin {alias}: cached commit is {actual}, expected {version}");
    }
    Ok(())
}

fn run_git(alias: &str, cwd: Option<&Path>, args: &[&str]) -> Result<()> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .output()
        .with_context(|| format!("plugin {alias}: run git"))?;
    if !output.status.success() {
        bail!(
            "plugin {alias}: git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn validate_manifest(
    alias: &str,
    definition: &PluginDefinition,
    manifest: &PluginManifest,
) -> Result<()> {
    if manifest.api_version != "vanityctl.dev/plugin/v1" {
        bail!(
            "plugin {alias}: unsupported apiVersion {:?}",
            manifest.api_version
        );
    }
    if manifest.version != definition.version {
        bail!(
            "plugin {alias}: requested version {}, manifest is {}",
            definition.version,
            manifest.version
        );
    }
    if !valid_name(&manifest.name) {
        bail!("plugin {alias}: manifest name is invalid");
    }
    if manifest.services.is_empty() {
        bail!("plugin {alias}: manifest must generate at least one service");
    }
    for key in manifest.services.keys() {
        if !valid_name(key) {
            bail!("plugin {alias}: generated service key {key:?} is invalid");
        }
    }
    Ok(())
}

fn resolve_inputs(
    instance: &str,
    manifest: &PluginManifest,
    supplied: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>> {
    for key in supplied.keys() {
        if !manifest.config.contains_key(key) {
            bail!("plugin service {instance}: unknown config key {key:?}");
        }
    }
    let mut resolved = BTreeMap::new();
    for (key, spec) in &manifest.config {
        let value = supplied.get(key).cloned().or_else(|| spec.default.clone());
        let Some(value) = value else {
            if spec.required {
                bail!("plugin service {instance}: missing required config key {key:?}");
            }
            continue;
        };
        let valid = matches!(
            (&spec.kind, &value),
            (InputType::String, Value::String(_))
                | (InputType::Boolean, Value::Bool(_))
                | (InputType::Integer, Value::Number(_))
        );
        if !valid {
            bail!("plugin service {instance}: config {key:?} has the wrong type");
        }
        resolved.insert(key.clone(), value);
    }
    Ok(resolved)
}

fn validate_secrets(
    instance: &str,
    manifest: &PluginManifest,
    supplied: &BTreeMap<String, String>,
) -> Result<()> {
    for key in supplied.keys() {
        if !manifest.secrets.contains_key(key) {
            bail!("plugin service {instance}: unknown secret key {key:?}");
        }
    }
    for (key, spec) in &manifest.secrets {
        if spec.required && !supplied.contains_key(key) {
            bail!("plugin service {instance}: missing required secret reference {key:?}");
        }
    }
    Ok(())
}

struct TemplateContext<'a> {
    instance: &'a str,
    directory: Option<&'a str>,
    config: &'a BTreeMap<String, Value>,
    secrets: &'a BTreeMap<String, String>,
}

fn render_value(value: &Value, context: &TemplateContext<'_>) -> Result<Value> {
    match value {
        Value::String(value) => render_string(value, context),
        Value::Sequence(values) => Ok(Value::Sequence(
            values
                .iter()
                .map(|value| render_value(value, context))
                .collect::<Result<_>>()?,
        )),
        Value::Mapping(values) => {
            let mut rendered = Mapping::new();
            for (key, value) in values {
                rendered.insert(key.clone(), render_value(value, context)?);
            }
            Ok(Value::Mapping(rendered))
        }
        _ => Ok(value.clone()),
    }
}

fn render_string(value: &str, context: &TemplateContext<'_>) -> Result<Value> {
    if let Some(token) = exact_token(value) {
        return lookup_token(token, context);
    }
    let mut rendered = value.to_string();
    while let Some(start) = rendered.find("${") {
        let tail = &rendered[start + 2..];
        let end = tail
            .find('}')
            .context("unterminated plugin template token")?;
        let token = &tail[..end];
        let replacement = lookup_token(token, context)?;
        let replacement = scalar_string(&replacement)
            .context("non-string plugin values must occupy the entire template field")?;
        rendered.replace_range(start..start + end + 3, &replacement);
    }
    Ok(Value::String(rendered))
}

fn exact_token(value: &str) -> Option<&str> {
    value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .filter(|value| !value.contains("${") && !value.contains('}'))
}

fn lookup_token(token: &str, context: &TemplateContext<'_>) -> Result<Value> {
    match token {
        "instance.name" => Ok(Value::String(context.instance.into())),
        "instance.directory" => Ok(Value::String(
            context
                .directory
                .context("plugin instance requires directory")?
                .into(),
        )),
        _ if token.starts_with("config.") => context
            .config
            .get(&token[7..])
            .cloned()
            .with_context(|| format!("plugin template references missing {token}")),
        _ if token.starts_with("secrets.") => context
            .secrets
            .get(&token[8..])
            .cloned()
            .map(Value::String)
            .with_context(|| format!("plugin template references missing {token}")),
        _ => bail!("unknown plugin template token {token:?}"),
    }
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn validate_template_secret_locations(
    alias: &str,
    value: &Value,
    path: &mut Vec<String>,
) -> Result<()> {
    match value {
        Value::String(value)
            if value.contains("${secrets.")
                && path.last().map(String::as_str) != Some("env_file") =>
        {
            bail!(
                "plugin {alias}: secret templates are only allowed in env_file, not {}",
                path.join(".")
            );
        }
        Value::Sequence(values) => {
            for value in values {
                validate_template_secret_locations(alias, value, path)?;
            }
        }
        Value::Mapping(values) => {
            for (key, value) in values {
                let key = key
                    .as_str()
                    .context("plugin template mapping keys must be strings")?;
                path.push(key.into());
                validate_template_secret_locations(alias, value, path)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_commit_pin(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn stdlib_catalog() -> Vec<Value> {
    let manifest: PluginManifest =
        serde_yaml::from_str(SUPABASE_SELFHOST).expect("bundled plugin is valid");
    vec![
        serde_yaml::to_value(BTreeMap::from([
            ("name", Value::String(manifest.name)),
            ("version", Value::String(manifest.version)),
            (
                "description",
                Value::String(manifest.description.unwrap_or_default()),
            ),
        ]))
        .expect("catalog serializes"),
    ]
}
