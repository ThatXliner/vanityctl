use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;
use vanityctl::{ConfigPaths, HostConfig};

#[test]
fn loads_main_file_and_service_fragments() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::create_dir_all(&paths.services).unwrap();
    fs::write(&paths.config, "version: 1\nservices:\n  web:\n    type: docker\n    image: nginx:alpine\n    ports: ['8080:80']\n").unwrap();
    fs::write(paths.services.join("jobs.yaml"), "services:\n  scraper:\n    type: job\n    command: /bin/echo\n    args: [hello]\n    schedule: '0 4 * * *'\n").unwrap();
    let config = HostConfig::load(&paths).unwrap();
    assert_eq!(config.services.len(), 2);
    assert_eq!(config.services["web"].ports, vec!["8080:80"]);
}

#[test]
fn rejects_invalid_and_unknown_configuration() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(&paths.config, "version: 1\nservices:\n  broken:\n    type: job\n    command: /bin/true\n    surprise: nope\n").unwrap();
    let error = format!("{:#}", HostConfig::load(&paths).unwrap_err());
    assert!(
        error.contains("unknown field") || error.contains("surprise"),
        "{error}"
    );
}

#[test]
fn rejects_public_unauthenticated_listener() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(
        &paths.config,
        "version: 1\napi:\n  listen: 0.0.0.0:7788\nservices: {}\n",
    )
    .unwrap();
    assert!(
        HostConfig::load(&paths)
            .unwrap_err()
            .to_string()
            .contains("non-loopback")
    );
}

#[test]
fn public_read_only_requires_a_token_for_mutating_routes() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(
        &paths.config,
        "version: 1\napi:\n  public_read_only: true\nservices: {}\n",
    )
    .unwrap();
    assert!(
        HostConfig::load(&paths)
            .unwrap_err()
            .to_string()
            .contains("requires api.token_env or api.token_file")
    );
}

#[test]
fn resolves_token_file_for_public_read_only_mode() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let token = dir.path().join("api-token");
    fs::write(&token, "secret-token\n").unwrap();
    fs::write(
        &paths.config,
        format!(
            "version: 1\napi:\n  token_file: {}\n  public_read_only: true\nservices: {{}}\n",
            token.display()
        ),
    )
    .unwrap();
    let config = HostConfig::load(&paths).unwrap();
    assert!(config.api.public_read_only);
    assert_eq!(
        config.api.resolve_token().unwrap().as_deref(),
        Some("secret-token")
    );
}

#[test]
fn accepts_legacy_and_ordered_compose_files() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("compose.yaml"), "services: {}\n").unwrap();
    fs::write(project.join("compose.production.yaml"), "services: {}\n").unwrap();
    fs::write(
        &paths.config,
        format!(
            "version: 1\nservices:\n  legacy:\n    type: compose\n    directory: {}\n    file: compose.yaml\n  multi:\n    type: compose\n    directory: {}\n    files:\n      - compose.yaml\n      - compose.production.yaml\n",
            project.display(),
            project.display()
        ),
    )
    .unwrap();

    let config = HostConfig::load(&paths).unwrap();
    assert_eq!(config.services["legacy"].compose_files(), ["compose.yaml"]);
    assert_eq!(
        config.services["multi"].compose_files(),
        ["compose.yaml", "compose.production.yaml"]
    );
}

#[test]
fn rejects_ambiguous_empty_and_missing_compose_files() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("compose.yaml"), "services: {}\n").unwrap();

    for (definition, expected) in [
        (
            "file: compose.yaml\n    files: [compose.yaml]",
            "either legacy file or files",
        ),
        ("files: []", "must not be empty"),
        ("files: [missing.yaml]", "is not readable"),
        (
            "files: [compose.yaml, compose.yaml]",
            "listed more than once",
        ),
    ] {
        let paths = ConfigPaths::from_root(dir.path().join(expected.replace(' ', "-")));
        fs::create_dir_all(&paths.root).unwrap();
        fs::write(
            &paths.config,
            format!(
                "version: 1\nservices:\n  app:\n    type: compose\n    directory: {}\n    {definition}\n",
                project.display()
            ),
        )
        .unwrap();
        let error = format!("{:#}", HostConfig::load(&paths).unwrap_err());
        assert!(
            error.contains(expected),
            "expected {expected:?} in {error:?}"
        );
    }
}

#[test]
fn rejects_compose_file_keys_on_other_service_types() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    fs::write(
        &paths.config,
        "version: 1\nservices:\n  web:\n    type: docker\n    image: nginx\n    files: [compose.yaml]\n",
    )
    .unwrap();
    assert!(
        HostConfig::load(&paths)
            .unwrap_err()
            .to_string()
            .contains("only valid for compose")
    );
}

#[cfg(unix)]
#[test]
fn dns_accepts_a_private_token_file() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let token = dir.path().join("cloudflare-token");
    fs::write(&token, "secret-token\n").unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(
        &paths.config,
        format!(
            "version: 1\ndns:\n  provider: cloudflare\n  zone_id: zone\n  token_file: {}\n  dynamic: [app.example.com]\nservices: {{}}\n",
            token.display()
        ),
    )
    .unwrap();
    HostConfig::load(&paths).unwrap();
}

#[cfg(unix)]
#[test]
fn dns_rejects_ambiguous_missing_and_public_token_files() {
    let dir = tempdir().unwrap();
    let public_token = dir.path().join("public-token");
    fs::write(&public_token, "secret-token\n").unwrap();
    fs::set_permissions(&public_token, fs::Permissions::from_mode(0o644)).unwrap();

    for (index, credentials, expected) in [
        (0, String::new(), "exactly one credential source"),
        (
            1,
            "  token_env: CLOUDFLARE_API_TOKEN\n  token_file: /tmp/token\n".into(),
            "exactly one credential source",
        ),
        (
            2,
            format!("  token_file: {}\n", public_token.display()),
            "must not be readable or writable by group/others",
        ),
    ] {
        let paths = ConfigPaths::from_root(dir.path().join(index.to_string()));
        fs::create_dir_all(&paths.root).unwrap();
        fs::write(
            &paths.config,
            format!(
                "version: 1\ndns:\n  provider: cloudflare\n  zone_id: zone\n{credentials}  dynamic: [app.example.com]\nservices: {{}}\n"
            ),
        )
        .unwrap();
        let error = format!("{:#}", HostConfig::load(&paths).unwrap_err());
        assert!(
            error.contains(expected),
            "expected {expected:?} in {error:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn concise_dns_expands_dynamic_hostnames_with_safe_defaults() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let token = dir.path().join("cloudflare-token");
    fs::write(&token, "secret-token\n").unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(
        &paths.config,
        format!(
            "version: 1\ndns:\n  provider: cloudflare\n  credentials: {}\n  dynamic:\n    - mc.example.com\n    - rustdesk.example.com\nservices: {{}}\n",
            token.display()
        ),
    )
    .unwrap();
    let config = HostConfig::load(&paths).unwrap();
    let dns = config.dns.unwrap();
    assert!(dns.zone_id.is_none());
    let records = dns.effective_records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].name, "mc.example.com");
    assert_eq!(records[0].value, "public_ip");
    assert!(!records[0].proxied);
}

#[cfg(unix)]
#[test]
fn native_env_files_must_be_private() {
    let dir = tempdir().unwrap();
    let paths = ConfigPaths::from_root(dir.path());
    let env_file = dir.path().join("worker.env");
    fs::write(&env_file, "API_TOKEN=secret\n").unwrap();
    fs::set_permissions(&env_file, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(
        &paths.config,
        format!(
            "version: 1\nservices:\n  worker:\n    type: process\n    command: /bin/true\n    env_file: {}\n",
            env_file.display()
        ),
    )
    .unwrap();
    assert!(
        format!("{:#}", HostConfig::load(&paths).unwrap_err())
            .contains("must not be readable or writable by group/others")
    );
}
