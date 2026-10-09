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

These always run in the CLI itself: `smf daemon …`, `smf mcp …`, `init`, `doctor`, `--help`,
`--version`; the daemon refuses them from any client (`cli_only`). Commands for an instance that
is not registered run directly.

If the daemon stops while it runs a command (crash, a Windows task ended outside `smf`, or a
command still running when the shutdown delay ends), a read runs again directly; a write is never
run twice: the CLI reports `daemon_interrupted`, and you check its result before running it
again. A shutdown lets commands already started finish and answer (65 s at most); a command
still waiting for its turn or for a place to run gets `daemon_stopping` and did not start (the
CLI runs a read again directly). Meanwhile new commands find no
daemon at once and run directly: on Linux and macOS the socket is removed; on Windows the pipe
stays listed but closes every new connection. `smf daemon stop`, `restart`, `install` and
`uninstall` wait for the daemon to exit (75 s at most) before they call the service manager, and
systemd and launchd give it as long before forcing it. After a second of waiting they say so on
stderr, and they warn when the delay runs out; with `--json`, stderr keeps only the JSON error. A
daemon that has just started and does not listen yet receives the stop request again until it
answers.

Under load, the daemon runs at most one command per processor core but one (two at least), each
on a thread of its own; connections, status and stop never wait for a place. A command has 60 s
from the moment it reaches the daemon: when its turn (a write behind other writes to the same
instance) or its place has not come by then, or comes with less than 3 s left, it gets
`daemon_busy` and did not start. A read waits 5 s at most for a place, then gets `daemon_busy`
too; the CLI runs a read again directly. The CLI waits 70 s for an
answer; past that, it treats the daemon as stopped during the command: a read runs again
directly, a write reports `daemon_interrupted` (the daemon may still be running it).
`smf daemon status` gives the daemon 5 s to answer, then reports it `running` with
`"answering": false`; such a daemon is replaced by `smf daemon restart` or `smf daemon install`.
The journal never holds up a command: lines are queued and written in the background.

| Command | Effect |
|---|---|
| `smf daemon status [--json]` | `absent`, `installed_stopped`, `running`, `version_mismatch`, `foreign_endpoint`, `service_manager_unavailable`; whether a running daemon answers, version, process, start, open instances, clients |
| `smf daemon start` / `stop` / `restart` | Pilots the installed service |
| `smf daemon register` / `unregister` | Adds or removes the instance found from the folder (or `--path`) |
| `smf daemon instances` | Registered instances; `missing` for one no longer found |
| `smf daemon run [--supervise]` | Serves in the foreground; the first Ctrl+C or SIGTERM lets the running commands finish, a second one stops at once |
| `smf daemon uninstall` | Stops and removes the service; instances, backups and registry are kept |

`smf doctor` reports the daemon: `ok` when it runs with your version, `warning` when it is absent
or stopped (commands run directly), `error` on `version_mismatch` or `foreign_endpoint`.

## Update smf

The service starts the `smf` of the tool, so an update needs a new start of the daemon: until
then it keeps the old version and the new CLI refuses to send it commands
(`daemon_version_mismatch`).

On Windows, the running daemon keeps the files of the tool in use and `dotnet tool update` fails
("Access to the path … is denied"), leaving the old version installed. An MCP server started by
Claude Code or Codex (`smf mcp serve`) runs the same tool: close those sessions too. Then:

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
outputs of a command. The daemon, the Windows supervisor and the MCP servers write it in turn,
through `daemon.log.lock` beside it; `instances.json.lock` does the same for changes to the
registry. Both stay in place and hold nothing. On Linux and macOS, these lock files and
`daemon.lock` are readable by you only (one left by an older version is narrowed), and a missing
configuration or state folder is created private to you; an existing folder keeps its rights.

On Windows, a terminal started by a packaged (MSIX) application, such as some desktop apps, may
write `%APPDATA%` into a private copy of that application: the registry it writes is then
invisible to the service, and `smf status` keeps running directly. Register from an ordinary
terminal; `smf daemon status` names the registry file the daemon reads.

## Environment variables

| Variable | Effect |
|---|---|
| `SERMOFUR_NO_DAEMON=1` | The CLI never tries the daemon |
| `SERMOFUR_DAEMON_HOME=<folder>` | Registry, journal and endpoint under this folder (tests, trials); the Windows scheduled task cannot carry it, so `install` refuses it there |

The variables recorded at install (`DOTNET_ROOT`, `PATH`) reach the daemon: through the unit or the
agent on Linux and macOS, through the supervisor on Windows. A different `PATH` alone does not make
`install` replace the service.

## Errors

| Code | Exit | When |
|---|---|---|
| `daemon_version_mismatch` | 3 | The running daemon has another version: `smf daemon restart` when it is the older one; otherwise restart the client (the MCP server in its host) or update smf |
| `daemon_unavailable` | 3 | Service not installed (`start`, `stop`, `restart`), or registered but not answering |
| `daemon_interrupted` | 3 | The daemon stopped during a write, or did not answer it within 70 s (it may still be running it): its result is unknown, check before running it again |
| `daemon_stopping` | 3 | The daemon was stopping; the command did not start: run it again |
| `daemon_busy` | 3 | The daemon could not start the command in time (writes ahead on the same instance, or every place taken: 5 s for a read, the 60 s deadline for a write); the command did not start: run it again (the CLI runs a read again directly) |
| `cli_only` | 3 | A client sent the daemon a command that runs only in the CLI |
| `daemon_already_running` | 3 | `smf daemon run` while a daemon already serves you |
| `service_manager_unavailable` | 3 | No service manager in the session |
| `service_install_failed` | 3 | The service manager refused the service, or a value cannot go into its definition; previous state restored: the previous registration is put back (on Windows, the previous task is rebuilt from its definition) and the previous daemon is started again if it was running (launchd starts it as it loads it) |
| `foreign_endpoint` | 4 | The endpoint or its folder belongs to another account or is not private |
| `invalid_registry` | 3 | Registry unreadable, too large or malformed; left untouched |
| `registry_busy` | 3 | Another `smf` process has been changing the registry for 10 s, or (Windows) another program holds it open; nothing changed: run again |
| `not_registered` | 1 | `unregister` of an instance that is not in the registry |
| `request_too_large` | 1 | Request over 256 KiB; the CLI runs it directly |
| `response_too_large` | 1 | Output of a write over 16 MiB: it ran, check its effect before running it again (a read runs again directly) |
| `request_timeout` | 3 | Command started but not finished 60 s after it reached the daemon; it keeps running and may still apply |
| `protocol_error` | 3 | Malformed request |
