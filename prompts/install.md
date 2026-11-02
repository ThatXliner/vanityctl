Help me set up vanityctl to manage my self-hosted machine, including the services
already running on it. Use the machine or SSH host I have authorized; if the
target is unclear, ask which host before connecting or changing anything.

Read https://github.com/ThatXliner/vanityctl, especially docs/onboarding.md and
skills/vanityctl/SKILL.md, then:

1. Inventory first. Check the OS, installed tools, existing vanityctl registry,
   Docker/Compose workloads, native services, scheduled jobs, startup owners,
   deployers, ports and persistent data. Keep credentials and environment values
   private. Discovery is read-only; a plist or stopped container is not proof of
   a running service.
2. Prepare a concrete migration plan. For each service, record its current owner,
   proposed registry entry, dependencies, health check and rollback. Separate
   additive registration from supervisor handoffs and restarts. Honor migration
   approval already given; ask only when a handoff or downtime is not authorized.
3. Install the supported tooling and daemon using the documented commands. Keep
   the API private, preserve existing configuration, and install the operations
   skill where my agent runs. macOS is the native-service target today; explain
   unsupported OS or service features rather than inventing a backend.
4. Bring services into the registry gradually. Preserve Compose project names,
   file order, image pins, volumes, environment files and existing deployers.
   Preview supported launchd adoption before executing a handoff. Avoid duplicate
   supervisors and global apply during discovery or incremental migration.
5. Verify each changed service locally and through its normal public route when
   applicable. Check real application behavior, not just a running process. Stop
   a failing migration and use its rollback before continuing with that service.
6. Leave a maintained machine registry and infrastructure repo, reusing an
   existing one where available. Keep secret values, raw private configs and
   backups outside public Git. Record ownership exceptions, recovery locations
   and health checks so later sessions can use vanityctl instead of rediscovering
   the machine. Do not publish or share the repo unless I authorize it.

Finish with what is managed, what remains with its existing owner and why, the
verification results, and the commands my agent should use next time.
