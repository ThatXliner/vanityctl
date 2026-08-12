# V0 architecture

`vanityctl` treats the YAML registry as desired state and keeps mutable observations
under a separate state directory. `hostd` is the only component that talks to
Docker, Compose, launchd, Git, or DNS providers. The CLI and the embedded dashboard
are API clients.

```text
config.yaml + services/*.yaml
             |
           hostd
    +--------+---------+----------+-------------+
    |        |         |          |             |
 Docker   Compose   launchd   Git deploys   DNS adapters
    |
 JSON state, owned generated files, logs, event history
    |
 localhost HTTP/JSON + SSE
    +-------------------+
    |                   |
 vanityctl          dashboard
```

## Ownership and safety

- Generated launchd labels use `dev.vanityctl.*` and files contain an ownership
  marker. The reconciler refuses to overwrite a file without that marker.
- Docker resources are labeled `dev.vanityctl.managed=true` and
  `dev.vanityctl.service=<name>`.
- Persistent volumes and source directories are never deleted by `apply`.
- Secrets are references (`env_file`, `token_env`), never returned by the API.
- The API binds to `127.0.0.1` by default and refuses a non-loopback listener unless
  an API token environment variable is configured.

## V0 boundary

V0 implements validated YAML, Docker containers, existing Compose projects,
launchd processes/jobs on macOS, unified lifecycle/log/status operations, idempotent
apply, Git deployment with per-service locking and polling, JSON state/history,
Cloudflare A/AAAA/CNAME reconciliation, doctor/agent context, and a read-mostly
dashboard. Linux/systemd, webhook receivers, rolling/blue-green deployment, reverse
proxy/TLS, dashboard config editing, and multi-host orchestration are deferred.

## Backend contract

Backends implement status, apply, lifecycle, and log operations. Compose additionally
supports ordered pull and build operations. Its effective file list is resolved from
the project directory and included in an on-disk reconciliation fingerprint, so a
second unchanged apply is a no-op while editing any override file causes reconciliation.
Command execution
is injected, which lets tests validate dispatch and generated commands without
touching real Docker or launchd resources. Deployment and DNS are coordinators above
the workload backends; neither is encoded as a special Docker behavior.
