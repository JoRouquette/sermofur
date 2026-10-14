English | [Français](../fr/adr/0016-ipc-protocol.md)

# ADR 0016-ipc-protocol — Local protocol 1 and routing of the CLI

Date: 2026-10-08, configuration writes added on 2026-10-12. Status: accepted,
implemented in 0.4 and 0.4.3. Completes
[ADR 0003](0003-ipc-daemon.md) and [ADR 0015](0015-user-daemon.md).

## Context
The CLI must give the same result with and without the daemon, and the daemon must apply exactly
the rules of the engine. The MCP bridge will be a second client of the same daemon.

## Decision
- Endpoint: a named pipe restricted to the current user on Windows (`PipeOptions.CurrentUserOnly`
  on both sides); a Unix domain socket `0600` in a folder `0700` elsewhere, the server checking
  the user ID of each peer (`SO_PEERCRED`, `getpeereid`), the client the owner and mode of the
  folder and of the socket. No network port. An endpoint of another account is `foreign_endpoint`.
- Frames: a little-endian `uint32` length, then a UTF-8 JSON body without ASCII escaping, at most
  256 KiB for what the daemon reads and 16 MiB for what a client reads (a list or an export). A
  session opens with `hello` (protocol, tool version, launch folder); the launch folder of the
  session never changes, and `--path` is honoured as in the direct CLI (the MCP bridge never sends
  it). `run` carries an id and the arguments; the daemon executes them with the same
  `CommandRunner` as the CLI, given the launch folder, and returns, under the same id, exit code,
  stdout and stderr as the CLI would have written them. An output over the limit is an error
  `response_too_large` with the id; the session goes on.
- The daemon checks the tool version first, then the protocol: a client of another version always
  gets `daemon_version_mismatch`, whatever protocol either side speaks.
- Writes to one instance run one at a time in the daemon; reads run concurrently. At most one
  command per processor core but one (two at least) runs at once, each on a thread of its own:
  hellos, status and shutdown never wait behind them. One deadline per request, from its arrival:
  a command whose turn or place has not come within the request timeout, or comes with less than
  a twentieth of it left, gets `daemon_busy` and did not start; a read waits 5 s at most for a
  place, then gets `daemon_busy` too (the CLI runs a read again directly; the MCP bridge, which
  never runs directly, passes the error to its host, which calls again); a started command not finished by then gets `request_timeout` and keeps running.
  The CLI gives the daemon the request timeout plus 10 s, then treats the session as lost: a read
  runs again directly, a write gets `daemon_interrupted`. The journal is written in the
  background and never holds up a request. A client that
  leaves cancels its queued command; a started command completes its transaction. A shutdown
  stops serving new clients at once (on Unix the socket is removed; on Windows, where a pipe name
  lives as long as its instances, one instance keeps accepting and closes every connection without
  a word, so a client concludes "no daemon" immediately), then lets started commands answer before
  the daemon exits (one request timeout plus 5 s); a command still waiting for its turn or its
  place gets `daemon_stopping`. A command still running when that delay ends is cut, and its transaction
  rolled back. A second Ctrl+C or SIGTERM ends the process at once. The daemon holds its lock file
  until it exits: `smf daemon stop`, `restart`, `install` and `uninstall` wait for that lock (even
  when the endpoint is already closed, sending the stop again to a daemon that starts listening
  late) before calling the service manager; systemd and launchd give the same delay; the Windows
  supervisor waits for it on a first Ctrl+C or SIGTERM, but a task ended outside `smf` (Task
  Scheduler, end of session) stops the daemon at once. Replays rely on the idempotency keys of the
  engine (`--key`), not on a cache of the daemon.
- `shutdown` is accepted before any `hello` and from any version, so that a newer `smf` can stop
  an older daemon.
- Routing in the CLI: `daemon …`, `mcp …`, `init`, `doctor`, help and version always run in the
  process, and the daemon refuses them from any client (`cli_only`). Otherwise, no endpoint →
  direct, with one file-system lookup; any refusal of the hello by the daemon (another version,
  another protocol) → the error, exit 3, never a direct run; an instance that is not registered →
  direct. Once a `run` is sent, a session lost without an answer is `daemon_interrupted`: a read
  runs again directly, a write is never run twice.
  `SERMOFUR_NO_DAEMON=1` forces direct runs; `SERMOFUR_DAEMON_HOME` moves configuration, state and
  endpoint under one folder (tests, trials).
- On Windows, the endpoint is looked up by listing the pipe folder: opening the pipe to read its
  attributes would consume an instance of the server.

## Configuration writes (0.4.3)
Every configuration file is replaced in one step: a temporary file of a random name in the same
folder, renamed over the target. A rename over a link replaces the link itself, so the real file is
never written; the Unix mode read through a link is the mode of its target, given to a plain file.
The files written this way do not have the same owner, so a write declares what a link in place of
its target means.
- **Refused** for the state of sermofur (`instances.json`, `service.json`): `unsafe_path` (exit 4),
  nothing written. No link there was put by the product, and following one would write outside the
  folder it owns. A symbolic link and a junction are what this sees; a hard link is another name of
  the same file, which no attribute tells apart, so a write through one replaces that name alone.
  Both writers hold this policy without being told to, and a test goes through each of them.
- **Followed** for the configuration of a host (`.mcp.json`, the `config.toml` of Codex): the chain
  of links is resolved and its final target is replaced, so the link stays a link and the file
  stays in the repository a manager of dotfiles put it in. Refusing would break a working
  installation; writing over the link would take the file out of that repository without a word.
- A followed chain is refused, by its own reason, when it leads nowhere, to a folder, in a circle,
  or through more than ten steps: writing anyway would leave a file the host does not read. A chain
  that comes back to a path already walked is named a circle; one that keeps going is named too
  long.
- A followed chain is followed wherever it leads: nothing requires its final target to stay in the
  project or in a folder of configuration. A link a repository carries can therefore send the write
  elsewhere, so the write is never silent about where it landed: `smf mcp install` and `uninstall`
  name the file actually replaced whenever it is not the one asked for.
- The Unix mode is read from, and given back to, the final target, never through the link.
- Only the target is looked at, not the folders above it: a folder of the path is a link on a
  healthy system (`/var` on macOS), and refusing those would refuse the daemon its own state. The
  discovery of an instance keeps refusing a link anywhere in its path (ADR 0010): that rule is
  about what may be read, not about how a file is replaced.
- One writer holds this (`AtomicFile`, in Infrastructure), and the instance writes of
  Infrastructure (configuration of an instance, Markdown projections) use it instead of their own
  temporary file.
- The supervisor passes the daemon the carried variables only (`DOTNET_ROOT`, `PATH`,
  `SERMOFUR_DAEMON_HOME`), with absolute paths only and `PATH` reduced to its absolute entries:
  `service.json` is a file of the user, and a relative path would be read from the working folder
  of the daemon. One journal line names each variable left out, never its value. A `service.json`
  whose shape does not hold reads as nothing installed, rather than failing outside the business
  errors.

## Alternatives
JSON-RPC (no gain while the CLI is the only client). Passing the folder through `--path` (clashes
with a `--path` already given). Running commands directly beside a daemon of another version (two
writers of different versions).

## Consequences
The cases of a refused or followed link chain, the permissions of a final target, the filtered
environment and an unsound `service.json` are covered by tests on the three operating systems; on
Windows a symbolic link to a file needs an elevated session or developer mode, so those facts are
reported as skipped without one.
A test replays real CLI commands with and without the daemon and compares outputs byte for byte;
a load test measures the overhead (2.9 ms at the 95th percentile on Windows).
