# vanityctl

**Agent-managed self-hosted hardware.**

One registry and CLI for the services running on your machine. Bring Codex,
Claude Code, or your preferred coding agent to set it up and maintain it—without
rediscovering Docker commands, startup scripts and deployment instructions each
session.

[Website](https://bryanhu.com/vanityctl/) · [Documentation](docs/README.md) · [How it compares](docs/comparison.md)

## Installation

Send this to your coding agent:

```text
Help me set up vanityctl on my self-hosted machine and bring my existing
services into its registry. Read and follow this onboarding prompt:
https://raw.githubusercontent.com/ThatXliner/vanityctl/main/prompts/install.md
Use the machine or SSH host I have authorized. Begin with a read-only
inventory and a concrete migration plan for the services already running.
```

Your agent inventories the host, installs the tooling, and helps migrate existing
services gradually. It preserves private configuration, checks application health,
and leaves operational knowledge for the next session.

Prefer to do it yourself? Follow the [manual installation guide](docs/guide.md#manual-installation).
Already have the CLI? `vanityctl install --prompt` prints the full setup prompt.

## Everyday commands

```console
vanityctl status
vanityctl logs my-app --lines 50
vanityctl up my-app
vanityctl restart my-app
```

For agents, `discover QUERY --json` finds existing workloads and
`agent-context NAME --json` supplies focused metadata and next commands.

## What it manages

- Docker containers and existing Compose stacks.
- Native processes and scheduled jobs through macOS launchd.
- Optional Git deployments and Cloudflare DNS reconciliation.

A local daemon handles execution. Humans and agents use the same CLI and API;
vanityctl does not require a hosted agent service or an LLM inside the daemon.

## Go deeper

- [Documentation](docs/README.md): installation, configuration, operations and troubleshooting.
- [Onboarding existing services](docs/onboarding.md): inventory, ownership, migration and rollback.
- [Agent workflows](docs/agent-workflows.md): recurring maintenance with less rediscovery.
- [Operator guide](docs/guide.md): the complete technical reference.

macOS-first and early-stage. Linux native-service management and systemd are
not implemented yet. See [current limitations](docs/guide.md#project-status-and-limitations).

[MIT license](LICENSE)
