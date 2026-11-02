---
name: vanityctl
description: Set up and operate agent-managed self-hosted hardware with vanityctl. Use for vanityctl installation, onboarding existing services, and its Docker/Compose workloads, launchd processes, jobs and deployments.
---

<!-- Managed by vanityctl install; edit the canonical skill in the vanityctl repo. -->

For initial setup or migration of existing services, use `vanityctl install
--prompt` when supported, or read
https://github.com/ThatXliner/vanityctl/blob/main/prompts/install.md.
Follow `docs/onboarding.md` in the vanityctl
checkout, or https://github.com/ThatXliner/vanityctl/blob/main/docs/onboarding.md
when no checkout is available. Keep inventory, additive registration and actual
supervisor handoffs distinct; use the operator's existing migration authorization.

Run these commands on the target host (over its existing SSH alias when remote).
Use the daemon's registry before searching app repos or recreating Docker/launchd
invocations. SSH is transport; vanityctl supplies the operational knowledge.

1. If the service name is known, run `vanityctl agent-context NAME --json`.
   Otherwise start with `vanityctl list --json`; use
   `vanityctl discover QUERY --json` for unregistered Docker/Compose resources
   and launchd labels. Search can match image names, directories and registered
   descriptions. Discovery is read-only, includes stopped/legacy resources, and
   does not prove they run or grant permission to adopt them.
2. Run `vanityctl status NAME --json` for current state, then use only the
   operation needed: `logs NAME --lines 50`, `restart NAME`, or `up NAME`.
   `up` creates/reconciles one service and re-reads Compose `.env`/env-file changes.
   `start` only resumes an existing resource. Global `apply` reconciles every
   service; it is not a discovery command. Preserve configured image pins and
   overlay order.
3. `agent-context` tells whether Git deployment is configured. Use `deploy NAME`
   and `deploy logs NAME` only for a configured Git deployment. Otherwise inspect
   the app's documented deployer; do not invent a competing one. Use `describe
   NAME --json` only when full configuration detail is needed; command arguments
   and source URLs in descriptions may contain private data.
4. After an authorized change, verify targeted status and the app's meaningful
   local/public health check. Running alone does not establish application health.
   Put persistent operational knowledge in the host's infrastructure repo and
   registry, so the next session starts with the same commands.

Credentials and environment-file contents stay private. Discovery and focused
context export only metadata, not command arguments, environment values or logs.
On a new Mac, `vanityctl install --start --codex-skill` sets up the daemon and
this skill. On an existing host it preserves the registry and user-written plist.
