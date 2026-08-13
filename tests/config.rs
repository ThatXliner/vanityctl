use std::fs;

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
