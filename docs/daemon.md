English | [Français](fr/daemon.md)

# Daemon

The daemon keeps your instances open for every client of your session: the CLI today, the MCP
bridge next. It runs under your account, as a service of your session, and only serves the
instances you register. Without it, `smf` works exactly as before. Design:
[ADR 0015](adr/0015-user-daemon.md), [ADR 0016](adr/0016-ipc-protocol.md).

## Install

```text
smf daemon install
smf daemon register          # from inside an instance, or with --path
smf status                   # "mode": "daemon"
```

`install` needs no administrator rights. It registers a service with the manager of your
session, starts it and waits for it to answer:

| System | Service | Restart after a crash |
|---|---|---|
| Windows | Scheduled task `\Sermofur\Daemon`, at logon, interactive token | within a second (supervisor) |
| Linux | `~/.config/systemd/user/sermofur.service` | after one second |
| macOS | `~/Library/LaunchAgents/io.github.jorouquette.sermofur.plist` | after one second |

Running `install` again changes nothing when the same version is installed; another version or
another `smf` executable replaces the service. Where no service manager exists (container, WSL
without systemd), `install` fails with `service_manager_unavailable`: run `smf daemon run` in a
terminal instead, or keep the CLI direct.

## Use

Once an instance is registered, every `smf` command run inside it goes through the daemon, with
the same output, errors and exit codes. The daemon is the only writer of the instance: concurrent
commands no longer wait and fail with `storage_busy`.

These always run in the CLI itself: `smf daemon …`, `init`, `doctor`, `--help`, `--version`.
Commands for an instance that is not registered run directly.

| Command | Effect |
|---|---|
| `smf daemon status [--json]` | `absent`, `installed_stopped`, `running`, `version_mismatch`, `foreign_endpoint`, `service_manager_unavailable`; version, process, start, open instances, clients |
| `smf daemon start` / `stop` / `restart` | Pilots the installed service |
| `smf daemon register` / `unregister` | Adds or removes the instance found from the folder (or `--path`) |
| `smf daemon instances` | Registered instances; `missing` for one no longer found |
| `smf daemon run [--supervise]` | Serves in the foreground, until Ctrl+C |
| `smf daemon uninstall` | Stops and removes the service; instances, backups and registry are kept |

`smf doctor` reports the daemon: `ok` when it runs with your version, `warning` when it is absent
or stopped (commands run directly), `error` on `version_mismatch` or `foreign_endpoint`.

## Update smf

The service starts the `smf` of the tool, so an update needs a new start of the daemon: until
then it keeps the old version and the new CLI refuses to send it commands
(`daemon_version_mismatch`).

On Windows, the running daemon keeps the files of the tool in use and `dotnet tool update` fails
("Access to the path … is denied"), leaving the old version installed. Stop it first:

```text
smf daemon stop
dotnet tool update --global Sermofur
smf daemon start
```

On Linux and macOS, files in use do not block their replacement: update, then run
`smf daemon restart` (this case is not covered by a test yet).

## Files

| | Windows | Linux | macOS |
|---|---|---|---|
| Registry `instances.json` | `%APPDATA%\Sermofur` | `$XDG_CONFIG_HOME/sermofur` | `~/Library/Application Support/Sermofur` |
| Journal `daemon.log`, `service.json` | `%LOCALAPPDATA%\Sermofur` | `$XDG_STATE_HOME/sermofur` | `~/Library/Logs/Sermofur` |
| Endpoint | named pipe `sermofur-<hash of your SID>` | `$XDG_RUNTIME_DIR/sermofur/daemon.sock` | `$TMPDIR/sermofur-<uid>/daemon.sock` |

The journal holds one JSON object per line (local time with its offset), three files of 1 MiB at
most. It records events, error codes, client process IDs and durations, never the arguments or
outputs of a command.

On Windows, a terminal started by a packaged (MSIX) application, such as some desktop apps, may
write `%APPDATA%` into a private copy of that application: the registry it writes is then
invisible to the service, and `smf status` keeps running directly. Register from an ordinary
terminal; `smf daemon status` names the registry file the daemon reads.

## Environment variables

| Variable | Effect |
|---|---|
| `SERMOFUR_NO_DAEMON=1` | The CLI never tries the daemon |
| `SERMOFUR_DAEMON_HOME=<folder>` | Registry, journal and endpoint under this folder (tests, trials) |

## Errors

| Code | Exit | When |
|---|---|---|
| `daemon_version_mismatch` | 3 | The running daemon has another version: `smf daemon restart` |
| `daemon_unavailable` | 3 | Service not installed (`start`, `stop`, `restart`), or registered but not answering |
| `daemon_already_running` | 3 | `smf daemon run` while a daemon already serves you |
| `service_manager_unavailable` | 3 | No service manager in the session |
| `service_install_failed` | 3 | The service manager refused the service; previous state restored |
| `foreign_endpoint` | 4 | The endpoint or its folder belongs to another account or is not private |
| `invalid_registry` | 3 | Registry unreadable, too large or malformed; left untouched |
| `not_registered` | 1 | `unregister` of an instance that is not in the registry |
| `request_too_large` | 1 | Request over 256 KiB |
| `request_timeout` | 3 | Command over 60 s; its transaction still completes |
| `protocol_error` | 3 | Malformed request |
