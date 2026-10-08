English | [Français](../fr/adr/0017-mcp-bridge.md)

# ADR 0017-mcp-bridge — MCP bridge as a client of the daemon

Date: 2026-10-08. Status: accepted, implemented in 0.4. Follows [ADR 0003](0003-ipc-daemon.md),
[ADR 0015](0015-user-daemon.md) and [ADR 0016](0016-ipc-protocol.md).

## Context
The tool names and rules of the MCP contract were fixed with 0.1: no SQL, no arbitrary file,
no scope chosen by the model, writes as auditable candidates. ADR 0003 forbids an engine per host.

## Decision
- `smf mcp serve` is an MCP server on stdio, built with the official C# SDK
  (`ModelContextProtocol.Core`, Apache-2.0, pinned), without the .NET Generic Host.
- Each tool becomes one or two `smf` commands (always `--json`) run by the daemon through local
  protocol 1, unchanged; the result is the JSON output of the command. The bridge never opens an
  instance: no daemon, a daemon of another version or an instance that is not registered are
  error results that name the remedy, and the server stays up.
- The scope is that of the launch folder: `CLAUDE_PROJECT_DIR` when the host sets it, the current
  folder otherwise. Schemas are closed and hold no path, scope, instance, origin or actor.
- The channel, not the tool, sets the provenance: origin `llm`, actor normalized from the client
  name of the initialization (`mcp-client` when empty). This is the "host channel" announced in
  the CLI documentation.
- `sermofur_feedback` checks that its target is visible, then records a draft RETEX
  `feedback <verdict> on <kind> <id>`: no format change, no effect on confidence or ranking;
  the Reflect/Learn lot will read these drafts.
- `smf mcp install` edits only the Sermofur entry of the host configuration, so that everything
  else stays byte for byte: the `sermofur` entry of `.mcp.json` for Claude Code (project scope;
  its user scope, `~/.claude.json`, is rewritten by Claude Code and left to `claude mcp add`), the
  `[mcp_servers.sermofur]` table and its sub-tables for Codex, in `.codex/config.toml` of the
  project or in the user configuration. The TOML edit has no full parser: it finds table headers
  outside strings and refuses what it cannot change safely.

## Alternatives
New IPC messages per tool (a second path to the engine to keep identical). An engine in the
bridge (forbidden by ADR 0003). A new record kind for feedback (instance format 3 and a
migration for every instance). `claude mcp add` (requires the Claude Code CLI and rewrites the
whole file). An in-house protocol implementation (declined).

## Consequences
The protocol is tested for real with the SDK client against `smf mcp serve` on the three systems,
and with Claude Code and Codex on a Windows workstation.
The feedback marker is textual until Reflect/Learn gives it a model.
