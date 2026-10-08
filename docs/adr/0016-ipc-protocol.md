English | [Français](../fr/adr/0016-ipc-protocol.md)

# ADR 0016-ipc-protocol — Local protocol 1 and routing of the CLI

Date: 2026-10-08. Status: accepted, implemented in 0.4. Completes
[ADR 0003](0003-ipc-daemon.md) and [ADR 0015](0015-user-daemon.md).

## Context
The CLI must give the same result with and without the daemon, and the daemon must apply exactly
the rules of the engine. The MCP bridge will be a second client of the same daemon.

## Decision
- Endpoint: a named pipe restricted to the current user on Windows (`PipeOptions.CurrentUserOnly`
  on both sides); a Unix domain socket `0600` in a folder `0700` elsewhere, the server checking
  the user ID of each peer (`SO_PEERCRED`, `getpeereid`), the client the owner and mode of the
  folder and of the socket. No network port. An endpoint of another account is `foreign_endpoint`.
- Frames: a little-endian `uint32` length, then a UTF-8 JSON body of at most 256 KiB. A session
  opens with `hello` (protocol, tool version, launch folder); the scope of the session comes from
  that folder and no request can change it. `run` carries the arguments; the daemon executes them
  with the same `CommandRunner` as the CLI, given the launch folder, and returns exit code, stdout
  and stderr as the CLI would have written them.
- Writes to one instance run one at a time in the daemon; reads run concurrently. A client that
  leaves cancels its queued command; a started command completes its transaction. Replays rely on
  the idempotency keys of the engine (`--key`), not on a cache of the daemon.
- `shutdown` is accepted before any `hello` and from any version, so that a newer `smf` can stop
  an older daemon.
- Routing in the CLI: `daemon …`, `init`, `doctor`, help and version always run in the process.
  Otherwise, no endpoint → direct, with one file-system lookup; a daemon of another version →
  `daemon_version_mismatch` (exit 3); an instance that is not registered → direct.
  `SERMOFUR_NO_DAEMON=1` forces direct runs; `SERMOFUR_DAEMON_HOME` moves configuration, state and
  endpoint under one folder (tests, trials).
- On Windows, the endpoint is looked up by listing the pipe folder: opening the pipe to read its
  attributes would consume an instance of the server.

## Alternatives
JSON-RPC (no gain while the CLI is the only client). Passing the folder through `--path` (clashes
with a `--path` already given). Running commands directly beside a daemon of another version (two
writers of different versions).

## Consequences
A test replays real CLI commands with and without the daemon and compares outputs byte for byte;
a load test measures the overhead (2.9 ms at the 95th percentile on Windows).
