# vanityctl

[Website](https://bryanhu.com/vanityctl/) · [Documentation](#quick-start)

`vanityctl` is a single-node declarative control plane for self-hosters and
developers. It answers one question:

> What should be running on this machine?

A Docker container, an existing Compose stack, a native daemon, and a scheduled
script are all services in one YAML registry and all use the same operational
vocabulary:

```console
vanityctl status
vanityctl restart minecraft
vanityctl deploy billion
vanityctl run scraper
vanityctl logs local-llm -f
```

This repository is a runnable V0, not a mockup. It includes the `hostd` daemon, the
`vanityctl` API client, an embedded local dashboard, macOS launchd integration,
Docker and Compose backends, Git deployment/polling, job history, JSON output, and
optional Cloudflare DNS reconciliation.

## Why Rust

Rust provides a small, low-overhead single binary; reliable typed configuration;
strong process and concurrency primitives; and mature CLI, YAML, and HTTP libraries.
It also makes it practical for `hostd`, the CLI, and the embedded UI to share one
model without requiring a runtime such as Node.js on the managed machine.

## Not Kubernetes

vanityctl assumes one machine. It has no nodes, pods, replica sets, cluster network,
distributed scheduler, or consensus layer. Docker still runs containers, Compose
still describes multi-container applications, and launchd still supervises native
processes. vanityctl is the boring layer above them that owns machine-level intent.

Use Coolify for a self-hosted app PaaS, Portainer for container administration, PM2
for application processes, or Kubernetes for distributed orchestration. vanityctl's
niche is a version-controlled whole-machine registry spanning all those workload
shapes without turning everything into a container.

For a detailed feature comparison and discussion of Coolify, Portainer, PM2,
Docker Compose, launchd/systemd, CasaOS, Umbrel, Kubernetes, and shell scripts, see
[How vanityctl differs from existing tools](docs/comparison.md).

## Current architecture

```text
~/.vanityctl/config.yaml + services/*.yaml
                    |
                  hostd
       +-------------+------------+------------+
       |             |            |            |
 Docker/Compose   launchd      Git deploys   Cloudflare DNS
       |             |            |            |
       +-------------+------------+------------+
                    |
        localhost HTTP/JSON + SSE
             +-------------+
             |             |
         vanityctl      dashboard
```

`hostd` alone performs orchestration. The CLI and dashboard consume its API. See
[the V0 architecture](docs/architecture.md) for ownership and safety boundaries.

## Installation

Requirements for V0:

- macOS (native processes/jobs use launchd)
- Rust 1.85 or newer to build from source
- Docker Desktop or another compatible Docker engine for Docker workloads
- Git for source-backed services

```console
git clone https://github.com/your-name/vanityctl.git
cd vanityctl
cargo install --path .
```

This installs `hostd` and `vanityctl`. Start `hostd` in a terminal while evaluating
V0:

```console
RUST_LOG=hostd=info hostd
```

For persistent use, create one small launchd agent for `hostd` itself. A packaged
self-installer is planned; vanityctl-generated workload plists do not require users
to author one plist per workload.

## Quick start

Create the machine registry:

```console
mkdir -p ~/.vanityctl/services
cp examples/machine/config.yaml ~/.vanityctl/config.yaml
cp examples/machine/services/*.yaml ~/.vanityctl/services/
vanityctl config validate
hostd
```

In another terminal:

```console
vanityctl doctor
vanityctl apply
vanityctl status
vanityctl dashboard
```

`vanityctl dashboard` prints the local dashboard URL (by default
<http://127.0.0.1:7788>). `apply` is idempotent: unchanged launchd definitions and
matching managed Docker container configuration are left alone. Compose `up -d` is
delegated to Compose, which performs its own reconciliation.

## Configuration

The default root is `~/.vanityctl`:

```text
~/.vanityctl/
├── config.yaml             # main desired state; safe to version-control
├── services/               # optional service fragments; safe to version-control
├── plugins/cache/          # pinned third-party plugin checkouts; do not edit
├── generated/              # owned launchd artifacts
├── logs/                   # service, job, and deployment logs
└── state/state.json        # observations and bounded history
```

Set `VANITYCTL_CONFIG=/path/to/infra/config.yaml` to use a checked-out infrastructure
repository elsewhere. Add `state/`, `logs/`, `generated/`, and `plugins/cache/` to that repository's
`.gitignore`. The main file and every fragment use strict parsing: misspelled or
unknown fields fail validation instead of being ignored.

A complete single-file registry is available at
[examples/full-config.yaml](examples/full-config.yaml). A minimal container is:

```yaml
version: 1
services:
  minecraft:
    type: docker
    image: itzg/minecraft-server
    ports: ["25565:25565"]
    volumes: ["~/data/minecraft:/data"]
    environment:
      EULA: "TRUE"
    restart: always
```

Reusable integrations can be declared as version-pinned, data-only plugins. The
built-in standard library starts with `supabase-selfhost`; local directories and
immutable Git commit sources are also supported. Plugins resolve to ordinary
services without install-time extension code. A plugin may also materialize one
commit-pinned upstream application repository into a missing or empty directory. See
[Declarative plugins](docs/plugins.md).

### Docker

`type: docker` supports an existing `image` or a `build` block, ports, bind/named
volumes, environment variables, `env_file`, command/arguments, and Docker restart
policies. Managed containers are named `vanityctl-<service>` and labeled with an
ownership marker and configuration hash. A changed Dockerfile workload builds the
new image before the old managed container is replaced. Persistent volumes are never
deleted by `apply`.

```yaml
billion:
  type: docker
  directory: ~/services/billion
  build:
    dockerfile: Dockerfile
    context: .
    args:
      APP_ENV: production
  ports: ["3000:3000"]
  env_file: ~/secrets/billion.env
  restart: unless-stopped
```

### Compose

Compose remains responsible for its application's internal topology:

```yaml
immich:
  type: compose
  directory: ~/services/immich
  files:
    - compose.yaml
    - compose.production.yaml
```

vanityctl runs the matching `docker compose` lifecycle and log commands from that
directory. File order is preserved for every command. The singular `file:` key is
still accepted for existing configurations, but new configurations should use
`files:`. Relative paths resolve from `directory`; validation fails before mutation
if any file is missing or unreadable.

`vanityctl pull immich` and `vanityctl build immich` expose Compose's corresponding
operations. A Git deployment pulls and builds before replacing the running stack.
Repeated `apply` calls skip unchanged Compose projects; changes to any configured
Compose file invalidate the reconciliation fingerprint.

### Native processes

```yaml
local-llm:
  type: process
  directory: ~/services/llm
  command: ./serve.sh
  args: ["--port", "11434"]
  restart: always
```

On macOS, `apply` generates an owned `dev.vanityctl.<name>` plist beneath
`~/.vanityctl/generated/launchd` and bootstraps it into the current user's launchd
GUI domain. stdout and stderr go to `~/.vanityctl/logs/<name>.log`. vanityctl refuses
to overwrite a generated-path file that lacks its ownership marker.

`command` is executed directly, not via an implicit shell. Use an explicit script
or `/bin/sh` plus arguments if shell behavior is actually needed.

Existing user LaunchAgents require an explicit, dry-run-first ownership handoff:

```console
vanityctl adopt launchd com.example.worker --as worker
vanityctl adopt launchd com.example.worker --as worker --execute
```

See [Adopting an existing launchd service](docs/launchd-adoption.md) for the
supported plist shape, duplicate-process protection, rollback, and limitations.

### Scheduled jobs

```yaml
scraper:
  type: job
  directory: ~/services/scraper
  command: ./scrape.sh
  schedule: "0 4 * * *"
```

V0 compiles exact daily times (`0 4 * * *`) and minute intervals
(`*/30 * * * *`) to launchd. Other cron shapes fail with a clear validation/apply
error rather than silently changing semantics. Manual runs record start time,
duration, exit code, and a log file:

```console
vanityctl jobs
vanityctl run scraper
vanityctl history scraper --json
vanityctl disable scraper
vanityctl enable scraper
```

Enable/disable is a runtime override. `vanityctl apply` restores the declarative
`enabled` value from YAML.

## Git deployment and automatic deploys

```yaml
billion:
  type: docker
  directory: ~/services/billion
  build: { dockerfile: Dockerfile }
  source:
    type: git
    repo: git@github.com:ThatXliner/billion.git
    branch: main
  deploy:
    auto: true
    strategy: pull
    trigger:
      type: poll
      interval: 60s
    before:
      - pnpm install --frozen-lockfile
    build:
      - pnpm build
    after:
      - echo deployed
```

Manual deployment fetches the configured branch, checks out its exact remote commit,
runs hooks, reconciles the workload, and records an immutable deployment entry and
log. Hooks are the explicit shell escape hatch and run with `/bin/sh -lc` in the
service directory.

```console
vanityctl deploy billion
vanityctl deploy billion --retry
vanityctl deploy history billion --json
vanityctl deploy logs billion
vanityctl deploy auto-disable billion
```

Polling uses `git ls-remote` and deploys only a new commit. Each service has a deploy
mutex, so deploys cannot overlap. One poll worker per Git service means rapid commits
coalesce to the latest observed commit. A failed commit is recorded and not retried
forever; use `--retry` or push a newer commit. The working service is not stopped
before hooks and Docker builds succeed.

Auto-enable/disable is also a runtime override. `apply` restores `deploy.auto` from
YAML. Webhook/GitHub trigger variants are reserved in the model but intentionally
rejected by V0 until signature verification and repository/branch checks are fully
implemented.

## DNS integration

Cloudflare credentials are referenced by environment variable and never returned by
`describe`, JSON status, or the dashboard:

```yaml
dns:
  provider: cloudflare
  zone_id: your-cloudflare-zone-id
  token_env: CLOUDFLARE_API_TOKEN
  interval: 5m
  records:
    - name: billion.example.com
      type: A
      value: public_ip
      proxied: true
```

```console
vanityctl dns status
vanityctl dns records --json
vanityctl dns reconcile
```

The reconciler resolves the public IP only when checking, compares desired records
with provider state, and writes only drifted records. `hostd` repeats reconciliation
at `dns.interval`, while the CLI action forces an immediate check. Registrar operations,
email DNS, DNSSEC, nameserver migration, and general Cloudflare administration are
out of scope.

## CLI and API

```text
vanityctl list [--json]
vanityctl status [SERVICE] [--json]       (alias: ps)
vanityctl describe SERVICE [--json]
vanityctl start|stop|restart SERVICE
vanityctl pull|build SERVICE              (Compose services)
vanityctl logs SERVICE [-f] [--lines N]
vanityctl apply
vanityctl apply --dry-run
vanityctl plugin [list|library]
vanityctl plugin describe INSTANCE
vanityctl deploy SERVICE [--retry]
vanityctl deploy history|logs SERVICE
vanityctl deploy auto-enable|auto-disable SERVICE
vanityctl jobs
vanityctl run SERVICE
vanityctl history SERVICE [--json]
vanityctl enable|disable SERVICE
vanityctl doctor [--json]
vanityctl config validate
vanityctl dns [status|records|reconcile]
vanityctl agent-context
vanityctl dashboard
```

The daemon API includes `/api/services`, per-service lifecycle/log/deployment routes,
`/api/jobs`, `/api/system`, `/api/events`, `/api/events/stream`, `/api/dns`, and
`/api/agent-context`. API errors are JSON. The browser dashboard is embedded in
`hostd`, refreshes machine state, offers workload-appropriate restart/run actions,
and shows redacted configuration and recent logs.

The API binds to `127.0.0.1:7788` by default. Configure `api.token_env` or
`api.token_file` to require a bearer token. For a public status dashboard while
keeping machine controls private, enable the explicit read-only allowlist:

```yaml
api:
  token_file: ~/.vanityctl/api-token
  public_read_only: true
```

This exposes status, host resource usage, jobs, activity, and DNS status. Logs,
configuration, agent context, and every mutating route remain authenticated. A
non-loopback listener still requires a token source. Do not assume a LAN is trusted.

The dashboard reports host CPU and RAM usage and, on Apple Silicon Macs, GPU
utilization and GPU-associated system memory when the graphics driver publishes
those counters. Docker and Compose rows report CPU and RAM; Compose values are
aggregated across the project's running containers.

## AI-agent integration

Stable JSON output and a redacted description keep agents from rediscovering deploy
instructions or exposing secrets:

```console
vanityctl list --json
vanityctl status --json
vanityctl describe billion --json
vanityctl agent-context > /tmp/vanityctl-context.md
```

A useful machine-level `AGENTS.md` is:

```markdown
All services on this machine are managed through vanityctl.
Do not manually start, kill, or replace managed workloads.
Before changing deployment behavior, run `vanityctl describe <service> --json`.
Use `vanityctl status`, `logs`, `restart`, and `deploy` for operations.
```

## Troubleshooting

- **Cannot reach hostd:** start `hostd`; confirm its logged listen address and that
  `VANITYCTL_CONFIG` is the same in both shells.
- **Docker check fails:** start Docker Desktop and run `vanityctl doctor` again.
- **launchd service is stopped:** inspect `vanityctl logs <name>` and
  `vanityctl status <name> --json`, then rerun `apply` after fixing configuration.
- **Deploy will not retry a commit:** inspect `vanityctl deploy logs <name>`, fix the
  cause, and explicitly run `vanityctl deploy <name> --retry`.
- **Config reports an unknown field:** correct the spelling. Strict schema errors are
  intentional.
- **DNS credential missing:** export the variable named by `dns.token_env` in the
  `hostd` environment; the value belongs outside version control.

## Project status and limitations

V0 is macOS-first. Linux/systemd is an architectural extension point but is not yet
implemented. Current limitations include: a deliberately small launchd cron subset;
no health-check schema or readiness gate; no blue/green port switching; no webhook
receiver; best-effort CPU/RAM metrics for Docker and
native PIDs; polling-based rather than push-based live logs; a read-only config UI;
and no packaged `hostd` installer. Compose reconciliation inherits Compose's own
idempotence. State uses a bounded atomic JSON file, which is appropriate for one
host but may move to SQLite as history/query volume grows.

The next reliability milestones are systemd, readiness checks, webhook signature
verification, a daemon self-installer, richer job next-run
calculation, and end-to-end macOS integration fixtures.

## Development

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
```

Tests use injected command runners and temporary roots, so they do not modify real
launchd jobs, containers, source directories, or DNS records.

Licensed under the MIT License.
