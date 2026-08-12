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
