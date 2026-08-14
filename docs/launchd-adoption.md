# Adopting an existing launchd service

vanityctl will not discover, unload, overwrite, or replace an existing launchd
service during `apply`. Adoption is a separate, explicit migration because starting
a second supervisor for the same command can corrupt data or bind the same port
twice.

## Inspect first

The current vertical slice adopts user LaunchAgents whose plist is exactly
`~/Library/LaunchAgents/<label>.plist`:

```console
vanityctl adopt launchd com.example.worker --as worker
```

The default is a dry run. It validates the source plist, generates and validates a
candidate service in memory, checks whether the label is loaded, and prints a
redacted handoff plan. It does not write, move, unload, or start anything.

Environment variable names are reported, but their values are never included in
human or JSON output. During execution, values from `EnvironmentVariables` move to
`~/.config/vanityctl/secrets/<service>.env` with mode `600`; YAML contains only the
file reference. Dry-run creates no secret file.

Review the command, arguments, working directory, scheduling, loaded state/PID, and
old log destinations. To perform the reviewed migration:

```console
vanityctl adopt launchd com.example.worker --as worker --execute
```

## Handoff and rollback

Before stopping the original, vanityctl validates both the service fragment and
the generated managed plist. The handoff then:

1. archives the original plist below `~/.vanityctl/adopted/launchd/`, without
   overwriting an existing archive;
2. writes an ownership-marked service fragment and
   `dev.vanityctl.<service>.plist`, plus a private environment file when needed;
3. unloads the original label;
4. bootstraps the managed label.

The old label is unloaded before the new label is loaded, so there is no duplicate
process window. A failed bootstrap removes any partially loaded managed label,
restores the original files, and reloads the original plist. If even that reload
fails, the error includes the exact recovery command; the source plist is still
restored in its original location.

After adoption succeeds, run `vanityctl config validate`, then inspect status and
logs normally. The archived source plist is retained as a rollback artifact and is
never deleted automatically.

## Supported launchd shape

The importer currently supports:

- an exact `Label` match;
- either `Program` or non-empty `ProgramArguments` (not both);
- `WorkingDirectory`;
- boolean `RunAtLoad`, boolean `KeepAlive`, and `KeepAlive.SuccessfulExit=false`;
- `StartInterval` when it is a whole number of minutes;
- one daily `StartCalendarInterval` containing only `Hour` and `Minute`;
- `StandardOutPath` and `StandardErrorPath` detection (managed logs move to
  vanityctl's log directory);
- string-valued `EnvironmentVariables`, migrated to a private file;
- `ThrottleInterval`, `ProcessType`, `LowPriorityIO`, and
  `SoftResourceLimits.NumberOfFiles`.

It deliberately refuses before mutation when it sees semantics it cannot preserve,
including other dictionary-form `KeepAlive` policies, sockets, Mach services,
queue/watch paths, unsupported resource limits, calendar arrays or weekday/month
schedules, simultaneous `Program` and `ProgramArguments`, and unknown plist keys.

Only current-user LaunchAgents are supported. System LaunchAgents, LaunchDaemons,
plists outside the exact conventional path, labels that are not currently loaded,
and ambiguous discovery require a manual migration. Adoption restarts the workload;
it guarantees an ordered handoff and rollback, not zero downtime.
