# Declarative plugins

Plugins package reusable operational knowledge without introducing another runtime.
During configuration loading, vanityctl validates each plugin instance and expands it
into ordinary Docker, Compose, process, or job services. The generated services then
use the same CLI, API, status, logs, apply, and ownership rules as handwritten
services.

V1 plugins are data-only YAML. The resolver runs no install-time plugin code and
writes only immutable Git checkouts beneath its cache. Generated workload commands
still run with the ordinary backend and permissions visible in the resolved service,
which is why third-party manifests must be reviewed. Template expansion has no
conditionals, loops, or shell evaluation. Secret references may only populate an
`env_file` field and their values are excluded from describe, plugin inspection,
dry-run, JSON, and agent context output.

## Using the standard library

The official library ships in the vanityctl binary and is versioned with the source
tree. `supabase-selfhost` is the first bundled integration:

```yaml
version: 1

plugins:
  supabase:
    source: stdlib:supabase-selfhost
    version: 1.0.0

services:
  sweepr-supabase:
    type: plugin
    plugin: supabase
    directory: ~/services/sweepr-supabase
    config:
      compose_file: docker-compose.yml
      backup_schedule: "0 2 * * *"
      database_service: db
    secrets:
      env_file: ~/.config/sweepr/supabase.env
```

The same complete example is available at
[`examples/plugins/supabase.yaml`](../examples/plugins/supabase.yaml).

This resolves to the Compose service `sweepr-supabase` and scheduled job
`sweepr-supabase-backup`. The backup uses `pg_dumpall`; stdout is retained in the
normal vanityctl job log. Treat that as a logical backup stream and export/rotate it
according to your retention needs. Restoring remains an explicit operator action in
this first plugin version because selecting and applying a database dump is
destructive.

Inspect the result before mutation:

```console
vanityctl plugin library
vanityctl plugin list --json
vanityctl plugin describe sweepr-supabase --json
vanityctl apply --dry-run
vanityctl apply
vanityctl status sweepr-supabase
vanityctl run sweepr-supabase-backup
```

## Local and Git sources

A local plugin is a directory containing `plugin.yaml`:

```yaml
plugins:
  media:
    source: path:plugins/media-stack
    version: 1.4.0
    integrity: sha256:... # optional, recommended for shared infra repositories
```

Relative paths resolve beneath the vanityctl configuration root. The requested
version must exactly match the manifest. If `integrity` is present, vanityctl hashes
the complete manifest bytes and refuses a mismatch.

Git sources use an immutable full commit SHA—branches and tags are rejected:

```yaml
plugins:
  media:
    source: git+https://github.com/example/vanityctl-plugin-media.git
    version: 1.4.0
    revision: 0123456789abcdef0123456789abcdef01234567
    integrity: sha256:...
```

Pinned Git plugins are checked out under `plugins/cache/<alias>/<commit>` inside the
configuration root. A cached checkout is verified on every load. Changing `version`
is an explicit upgrade; vanityctl never follows a moving branch or silently updates
a plugin.

## Authoring a plugin

`plugin.yaml` uses the following shape:

```yaml
apiVersion: vanityctl.dev/plugin/v1
name: static-web
version: 1.0.0
description: One reusable web container
config:
  image:
    type: string
    required: true
  port:
    type: integer
    default: 8080
secrets:
  env_file:
    required: true
services:
  main:
    type: docker
    image: "${config.image}"
    ports: ["${config.port}:80"]
    env_file: "${secrets.env_file}"
upgradeGuidance: Review image release notes before changing the pin.
removalGuidance: Removing this declaration never deletes persistent data.
```

Supported input types are `string`, `boolean`, and `integer`. Templates can refer to
`${instance.name}`, `${instance.directory}`, `${config.key}`, and `${secrets.key}`.
The service key `main` adopts the plugin instance name; additional keys become
`<instance>-<key>`. Generated names must not collide with handwritten services.

Plugin manifests should generate only resources necessary for the integration.
They cannot claim resources outside the generated service names. Shell behavior is
not implicit; any command is represented with a command plus argument array under
the ordinary service schema.

## Trust, upgrades, and removal

Review third-party manifests before use. Data-only does not mean harmless: a plugin
can still declare containers and commands that receive the permissions you grant
them. Prefer the bundled library or a source pinned by both commit and integrity.

Upgrade by changing the explicit version/integrity, running `config validate` and
`apply --dry-run`, reviewing the generated resources and guidance, then applying.
Removing a plugin declaration stops it from participating in future reconciliation.
vanityctl never deletes project directories, bind mounts, Docker volumes, database
data, cached job logs, or backups as part of plugin removal.
