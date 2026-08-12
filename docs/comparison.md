# How vanityctl differs from existing tools

There are already excellent tools for deploying applications, managing containers, supervising processes, and running home servers.

vanityctl is not intended to replace all of them feature-for-feature.

It exists because they generally start from a different abstraction.

Most existing tools ask questions like:

> How do I manage these containers?

> How do I keep this process alive?

> How do I deploy this web application?

> How do I orchestrate this cluster?

vanityctl starts with a different question:

> **What should be running on this machine?**

A service might be a Docker container, a Compose stack, a native binary, a scheduled script, a Git-backed application, or another host-level workload.

The goal is to represent all of them through one declarative model and one operational interface.

```text
                       Existing tools

             containers       processes
                 │                │
          Portainer/Coolify      PM2
                 │                │
                 └──────┬─────────┘
                        │
                 operating system
                        │
                 launchd/systemd


                         vanityctl

                    desired state
                         │
                       hostd
                         │
         ┌───────────────┼────────────────┐
         ▼               ▼                ▼
      Docker         native process    scheduled job
         │               │                │
      Compose          launchd           launchd
                         │
                    host machine
```

### Comparison

| Tool                  | Best at                                       | Containers | Native processes |     Scheduled jobs | Git auto-deploy | Declarative whole-machine state |        Web UI | Agent-oriented CLI/API |
| --------------------- | --------------------------------------------- | ---------: | ---------------: | -----------------: | --------------: | ------------------------------: | ------------: | ---------------------: |
| **vanityctl**         | Single-machine self-hosting control plane     |          ✓ |                ✓ |                  ✓ |               ✓ |                           **✓** |             ✓ |                  **✓** |
| **Coolify**           | Self-hosted application PaaS                  |          ✓ |               —* |                  ✓ |               ✓ |                         Partial |             ✓ |                    API |
| **Portainer**         | Container management                          |          ✓ |                ✗ | Container-oriented |        ✓/GitOps |                               ✗ |             ✓ |                    API |
| **PM2**               | Application process supervision               |          ✗ |                ✓ |            Limited |         Limited |                               ✗ |       Limited |                    CLI |
| **Docker Compose**    | Defining multi-container applications         |          ✓ |                ✗ |                  ✗ |               ✗ |                               ✗ |             ✗ |                    CLI |
| **launchd / systemd** | OS service supervision                        |   Indirect |                ✓ |                  ✓ |               ✗ |                       Low-level |             ✗ |                    CLI |
| **CasaOS**            | Friendly home-server applications             |          ✓ |                ✗ |            Limited |               ✗ |                               ✗ |             ✓ |          Not the focus |
| **Umbrel**            | Consumer-friendly home server / app ecosystem |          ✓ |                ✗ |       App-specific |    App-specific |                               ✗ |             ✓ |          Not the focus |
| **Kubernetes**        | Distributed container orchestration           |          ✓ |              ✗** |                  ✓ |   Via ecosystem |                               ✓ | Via ecosystem |                      ✓ |

* Coolify's application deployment model is fundamentally container-backed.
** Kubernetes can technically wrap host-level workloads through various mechanisms, but arbitrary native host processes are not its normal workload abstraction.

Feature matrices necessarily simplify each project's capabilities. This comparison is about their primary abstraction and intended workflow rather than whether a feature can technically be achieved with plugins, scripts, or workarounds.

### Coolify

Coolify is probably the closest existing project conceptually.

It is an open-source self-hosted PaaS that can deploy applications and services using Docker-based build/deployment mechanisms. It supports Git integrations and automatic deployments, Docker Compose applications, domains/proxies, SSL, monitoring, and scheduled automation.

If your goal is:

> "I have web applications and databases and want my own Heroku/Vercel."

Coolify is likely the better choice.

vanityctl is aimed at a slightly different problem:

> "I have a computer running many unrelated things, and I want one system that understands all of them."

That distinction matters when a machine contains things like:

```text
Next.js website
Minecraft server
Ollama/LLM server
Python scraper
Cloudflare DDNS updater
backup script
native daemon
Docker service
scheduled maintenance task
```

The goal is that these should all appear as peers:

```bash
vanityctl status
```

rather than some existing in Coolify, some in launchd, some in cron, and others as manually managed processes.

Another intentional distinction is configuration ownership.

Coolify stores and operates on configuration through its own control plane/database; its documented control path reads saved configuration and executes Docker operations against target servers.

vanityctl instead aims for the machine's desired state to remain representable as ordinary version-controlled configuration:

```text
~/infra/
    host.yaml
    services/
        minecraft.yaml
        billion.yaml
        scraper.yaml
        ddns.yaml
```

That makes rebuilding the machine, reviewing infrastructure changes, and giving coding agents operational context straightforward.

### Portainer

Portainer is primarily a management interface and control plane for container environments. Its Community Edition manages Docker resources such as containers, images, networks, volumes, and stacks, and Portainer supports Compose-based stacks and webhook/GitOps-oriented deployment workflows.

If the problem is:

> "I have lots of Docker resources and want a good interface for managing them."

Portainer is a strong fit.

vanityctl deliberately does not make the container the top-level abstraction.

A native executable:

```yaml
type: process
command: ./server
```

and a Docker workload:

```yaml
type: docker
image: example/server
```

should participate in the same lifecycle:

```bash
vanityctl start
vanityctl stop
vanityctl restart
vanityctl logs
```

Portainer manages **container infrastructure**.

vanityctl aims to manage **things running on one machine**, regardless of how they run.

### PM2

PM2 is a daemon process manager, particularly associated with Node.js applications. It provides process supervision, restart behavior, logging, monitoring, startup integration, and features such as clustering and zero-downtime reloads.

PM2 is excellent if the problem is:

> "Keep these application processes alive and make them easy to restart."

vanityctl borrows heavily from PM2's excellent operational simplicity:

```bash
pm2 list
pm2 restart foo
pm2 logs foo
```

becomes conceptually:

```bash
vanityctl list
vanityctl restart foo
vanityctl logs foo
```

The difference is scope.

`foo` might not be an application process at all.

It could be:

```text
Docker container
Compose project
native process
scheduled job
Minecraft server
Git deployment
DNS reconciler
```

The consistent CLI experience of PM2 is something vanityctl should emulate while applying it to a broader single-machine workload model.

### Docker Compose

Docker Compose defines and runs multi-container applications through declarative YAML describing services, networks, and volumes.

Compose solves:

> "How should the containers belonging to this application run?"

It intentionally does not solve:

> "What is everything this computer is responsible for?"

vanityctl therefore does not replace Compose.

Compose should be treated as one supported backend:

```yaml
services:
  immich:
    type: compose
    directory: ~/services/immich
    file: compose.yaml
```

The Compose file remains responsible for the internal architecture of that application.

The host control plane becomes responsible for understanding that `immich` is one workload on the machine.

### launchd and systemd

`launchd` and `systemd` are operating-system service managers.

systemd describes itself as a system and service manager and supervises processes through service units.

These tools are extremely capable and should not be unnecessarily replaced.

Instead, vanityctl should use them.

For example:

```yaml
services:
  ddns:
    type: job
    command: ./update-dns
    schedule: "*/30 * * * *"
```

might compile into an owned launchd definition on macOS.

The important difference is the level of abstraction.

Without vanityctl:

```text
write plist
install plist
remember plist location
launchctl bootstrap ...
launchctl kickstart ...
find logs
remember service identifier
```

With vanityctl:

```bash
vanityctl apply
vanityctl status ddns
vanityctl run ddns
vanityctl logs ddns
```

`launchd` or `systemd` remains the reliable underlying supervisor.

vanityctl provides the human-facing declarative control plane above it.

### CasaOS and Umbrel

CasaOS and Umbrel focus on making home servers approachable through graphical interfaces and app ecosystems.

CasaOS prominently provides Docker-based self-hosted applications through an app-store-style experience.

Umbrel describes umbrelOS as an operating system for home servers with a graphical app ecosystem aimed at making self-hosting accessible without substantial technical knowledge.

Their target experience is approximately:

> "I want to install Jellyfin, Immich, Nextcloud, or another packaged self-hosted app."

vanityctl's target experience is closer to:

> "I build and operate my own software on this machine."

It therefore prioritizes:

```text
Git repositories
deployment lifecycle
native executables
Docker workloads
scheduled jobs
machine configuration as code
stable CLI/API behavior
AI coding agents
```

over an application marketplace.

### Kubernetes

Kubernetes is a production-grade orchestration system for automating deployment, scaling, and management of containerized applications.

It solves a much larger problem:

```text
many workloads
       ×
many machines
       ×
failure handling
       ×
networking
       ×
scheduling
       ×
scaling
```

vanityctl intentionally assumes:

```text
one machine
```

That removes entire categories of distributed-systems complexity.

There is no need for:

```text
nodes
pods
replica sets
cluster schedulers
cluster networking
service discovery layers
distributed consensus
```

just to say:

```yaml
minecraft:
  type: docker
  image: itzg/minecraft-server
  restart: always
```

Kubernetes is appropriate when the infrastructure itself is distributed.

vanityctl is intentionally optimized for the increasingly common situation where one powerful home server, workstation, Mac Studio, NUC, or VPS hosts many independent workloads.

### Why not just scripts?

Shell scripts work extremely well initially:

```bash
./deploy.sh
./restart.sh
./update-ddns.sh
```

The problem appears when every project develops its own operational vocabulary.

One repository uses:

```bash
docker compose up -d --build
```

another uses:

```bash
pnpm build && pm2 restart server
```

another requires:

```bash
launchctl kickstart ...
```

and another has a bespoke deployment script nobody remembers six months later.

Humans can rediscover those details.

Coding agents can rediscover those details too — but doing so consumes context, commands, time, and introduces opportunities for mistakes.

vanityctl attempts to move that knowledge from:

```text
human memory
README fragments
AGENTS.md
shell history
LLM context
```

into structured machine-readable state.

Then an agent only needs to know:

```bash
vanityctl describe billion
vanityctl deploy billion
```

### The niche

vanityctl therefore sits roughly here:

```text
                     ONE MACHINE                    MANY MACHINES
                          │                              │
                          │                              │
Processes ───── PM2 ──────┤                              │
                          │                              │
OS services ─ launchd ────┤                              │
              systemd     │                              │
                          │                              │
Containers ─ Portainer ───┤                         Kubernetes
                          │                              │
PaaS ─────── Coolify ─────┤                              │
                          │                              │
Home server ─ CasaOS ─────┤                              │
              Umbrel      │                              │
                          │
                          │
                  ┌───────▼────────┐
                  │   VANITYCTL    │
                  │                │
                  │ declarative    │
                  │ whole-machine  │
                  │ control plane  │
                  └────────────────┘
```

The objective is **not** to be the smallest Kubernetes.

It is not a container dashboard.

It is not a replacement init system.

It is not another PaaS.

It is a **single-node declarative control plane for self-hosters and developers**.

The underlying technologies remain useful:

```text
Docker runs containers.
Compose defines container applications.
launchd/systemd supervise host processes.
Git stores source code.
DNS providers manage DNS.
```

vanityctl provides the missing layer that says:

> **Here is everything this machine is responsible for, here is its desired state, and here is one consistent way to operate it.**

