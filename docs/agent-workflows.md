# Agent-managed hardware: the maintenance loop

For installation or migration of existing services, start with the
[setup prompt](../prompts/install.md) and [onboarding guide](onboarding.md).
Once the host is onboarded, use operational metadata for recurring maintenance.
The CLI uses hostd's registry and the same backend dispatch for each workload.

## Known service

```sh
vanityctl agent-context app --json
vanityctl status app --json
vanityctl logs app --lines 50
```

Focused context gives the service type, directory, Compose file order, whether
Git deployment is configured, and command arrays for the next operation. It
omits environment values, arguments and source URLs. A one-service status probes
only that backend; it does not first poll every unrelated stack on the host.
Use `describe` when deeper configuration detail is actually needed, and keep
its potentially private arguments/source locations out of public reports.

## Unknown name or an existing unmanaged workload

```sh
vanityctl list --json
vanityctl discover search-term --json
```

Discovery searches resource names, images, directories, registered names and
their descriptions. A Compose project is grouped into one resource with its
containers and observed file inputs. Launchd results show the plist and user or
system domain. Empty `managedAs` means it is not registered with vanityctl.
Stopped containers and installed legacy plists are included; discovery is not
a health check. Docker being unavailable is reported as a warning while launchd
discovery continues.

This read-only command performs no adoption, process launches, Git changes or
workload restarts. Confirm ownership before using `adopt launchd`. Register
existing Compose inputs in the host registry while preserving overlay order,
project naming and any existing application-specific deployer.

## Initial creation or a configuration/environment change

Edit the intended service configuration or its private environment file, then:

```sh
vanityctl up app --json
vanityctl status app --json
```

`up` reconciles exactly one service. For Compose it invokes `up -d` with the
registered input files, allowing Compose to compare the resolved configuration
including `.env` and referenced environment files. It does not substitute a
global `apply`, force-recreate every container, pull new image tags, or build an
unrelated service. The command also works for configured Docker/native services
through their existing reconciliation backend. Declaratively disabled services
remain disabled. Use `start` to resume an already-created resource and `restart`
to restart it without re-reading changed Compose environment inputs.

## Deployment and verification

If focused context says Git deployment is configured:

```sh
vanityctl deploy app
vanityctl deploy logs app
vanityctl status app --json
```

Otherwise follow the app's existing deployer and document that integration in
the host infrastructure repo. Registering an existing Compose stack does not
silently replace its GitHub Actions job or external auto-deployment agent.

After any change, run the application's meaningful local/public health check.
This remains application-specific: a running container is insufficient evidence
that a login flow, uploaded file, or proxied page works. Retain those checks in
the infrastructure repo instead of writing fresh one-off verification each time.

## Onboarding agents

On a service host, `vanityctl install --start --codex-skill` creates the daemon
setup and installs `skills/vanityctl/SKILL.md` into Codex. On a separate agent
client, copy/link that same skill into its Codex skills directory. The skill
directs agents to the commands above before broad host searches or custom shell
wrappers. Keep host-specific exceptions and private recovery pointers in the
host's infrastructure repo; keep reusable operational behavior in vanityctl.
