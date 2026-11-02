# Onboard a machine with your agent

vanityctl is the operational layer for **agent-managed self-hosted hardware**.
Your coding agent inventories the host, prepares the migration and maintains its
registry. The CLI and hostd provide the inspectable configuration and execution
path. Bring Codex, Claude Code, or another agent that can read the repository and
run commands on your authorized host; vanityctl does not require a hosted agent
service or an LLM inside its daemon.

The entry point is a prompt. Copy [the setup prompt](../prompts/install.md) into
your agent and identify the target machine or SSH alias. If you already have the
CLI, `vanityctl install --prompt` prints the same prompt without connecting to
hostd, creating configuration, or changing the machine. The prompt helps the
agent bring existing workloads into the registry, as well as install the tool.

## What onboarding leaves behind

- A supported installation of vanityctl and hostd, with a private local API.
- A registry of the workloads actually brought under management, preserving
  their storage, startup semantics and deployment ownership.
- Operational instructions that agents can use through focused context,
  discovery and targeted lifecycle commands.
- A machine-specific infrastructure repo with health checks, recovery pointers
  and explicit exceptions. Secret material stays outside public history.

The inventory and migration plan precede workload changes. Installing the daemon
does not authorize a mass restart or takeover of every process on the machine.
Use existing approval for the intended migration; resolve any unapproved
ownership change or downtime before that handoff. Discovery does not adopt
resources. Global `apply` reconciles the entire registry and belongs after the
operator has reviewed its intended state.

## Installation is a step the agent can execute

The [manual installation instructions](../README.md#manual-installation) remain
available for operators and automation. On a new supported Mac, the agent can
build the CLI/daemon and use:

```sh
vanityctl install --start --codex-skill
```

This creates the private API configuration, hostd user agent and optional Codex
skill. It preserves existing configuration and compatible daemon setup; it does
not migrate or restart application workloads. The CLI installer currently targets
macOS launchd. Docker/Compose discovery and the rest of the architecture do not
make Linux native-service installation or systemd adoption implemented features.

Install the skill on the coding-agent client too when it is separate from the
service host. The reusable skill supplies the workflow; the machine's own
infrastructure repo supplies its hostnames, service map and exceptions.

## Moving existing services

### Compose stacks

Use `vanityctl discover QUERY --json` to locate existing projects. Cross-check
their original input files and current owner before creating a service fragment:

```yaml
services:
  my-app:
    type: compose
    directory: ~/services/my-app
    files:
      - compose.yaml
      - compose.production.yaml
```

Preserve the original project name, file order and working directory. In the
current schema the effective project name comes from Compose itself, such as
the files' `name:` or the existing `COMPOSE_PROJECT_NAME` configuration. If the
old launcher supplied `-p`, establish and verify an equivalent project name
before running lifecycle commands. A matching directory alone is insufficient.
Container labels are observations and may omit another owner's overlay.

Registering the existing inputs adds operational knowledge without recreating
containers. Validate the registry, inspect `agent-context NAME --json` and
`status NAME --json`, then use `up NAME` only for an intended reconciliation.
The existing app's auto-deployer can remain its deployment owner. Avoid enabling
a second polling deployer just because the stack is now registered.

### Native processes and scheduled jobs

Use [launchd adoption](launchd-adoption.md) for supported, loaded current-user
LaunchAgents. The first command previews the handoff:

```sh
vanityctl adopt launchd com.example.worker --as worker
```

Review that specific plan and its rollback before adding `--execute`. Adoption
restarts the workload and performs an ordered supervisor handoff. Unsupported
plist semantics, system daemons and unloaded legacy definitions stay with their
existing owner until an explicit supported migration is prepared. Record that
exception rather than converting it approximately or starting another supervisor.

### Standalone containers and external automation

Discovery also shows standalone containers, but there is no automatic Docker
adoption command today. A new `type: docker` definition uses vanityctl's managed
container name; applying it does not import an arbitrary existing container.
Plan storage, naming and a controlled handoff before replacement. Preserve local
security rebuilds and their sources instead of substituting upstream image tags.

A cron entry, external auto-deployer or system daemon is still part of the
inventory even when it remains outside vanityctl ownership. Preserve its schedule
and owner, and record the exact inspection and recovery procedure in the host
repo. Coverage should be explicit, not inferred from how many entries exist.

## Verify and make the next session cheaper

For every migrated service, verify its live state and meaningful application
behavior. Check the real local/public route, job result, database access or file
operation as appropriate. Verify that the previous supervisor cannot start a
duplicate. Keep a tested rollback and private recovery material available.

Finish with a coverage report: managed workloads, workloads deliberately left
with their existing owners, unsupported migrations, health checks and remaining
work. Never call an incomplete migration complete just because the daemon starts.

Future sessions should begin with the [agent workflows](agent-workflows.md):

```sh
vanityctl agent-context my-app --json
vanityctl status my-app --json
```

If a name is unknown, use `list --json` and `discover QUERY --json`. Save new
operational knowledge in the registry and host repo so each agent session builds
on the last one.
