English | [Français](../fr/adr/0015-user-daemon.md)

# ADR 0015-user-daemon — One daemon per user, as a service of the session

Date: 2026-10-08. Status: accepted, implemented in 0.4. Replaces the "machine daemon" of
[ADR 0003](0003-ipc-daemon.md); its local transport stays.

## Context
ADR 0003 planned a machine daemon multiplexing registered instances. Since
[ADR 0014](0014-instance-owner.md), an instance of another account is never read: a daemon
running under a system account would be refused by the very check that protects the user, and the
transport restricted to the current user only makes sense when the server is that user.

## Decision
- One daemon per user and per machine, running under the user's own account. It serves only the
  instances the user registered with `smf daemon register` (registry in the user configuration
  folder, 64 KiB at most, replaced atomically, never rewritten when it cannot be read).
- It is installed as a service of the user session, without administrator rights:
  - Windows: a scheduled task started at logon with an interactive token. S4U is not used: outside
    a domain it does not start the task. The task scheduler restarts a task after one minute at
    best, so the task runs `smf daemon run --supervise`, which restarts the daemon within a second
    and keeps it in a job object so that it never outlives the supervisor.
  - Linux: a `systemd --user` unit, `Restart=on-failure`, `RestartSec=1`.
  - macOS: a launchd agent, `RunAtLoad`, `KeepAlive` on failure.
- The service starts the `smf` that installed it. After `dotnet tool update`, `smf daemon restart`
  loads the new version; a CLI of another version than the daemon refuses to run commands
  (`daemon_version_mismatch`) instead of writing beside it.
- Uninstalling removes the service only: instances, backups and the registry are kept.
- Without a service manager (container, WSL without systemd), installation fails with
  `service_manager_unavailable`; the CLI keeps working directly and `smf daemon run` serves in
  the foreground.

## Alternatives
A machine service (administrator rights, contradicts ADR 0014). A daemon started on demand by the
CLI (no restart after a crash, no start with the session). A `Run` registry key or cron
`@reboot` (no restart, not tied to the session).

## Consequences
The real installation is tested in CI on the three systems; on a developer machine that test only
runs when asked (`SERMOFUR_SERVICE_TESTS=1`). If the supervisor itself dies on Windows, the task
scheduler brings it back after a minute.
