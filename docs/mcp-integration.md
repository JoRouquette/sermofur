English | [Français](fr/mcp-integration.md)

# MCP integration

`smf mcp serve` is an MCP server on stdio for Claude Code and Codex. It has no memory engine of
its own: every tool runs as an `smf` command through the [daemon](daemon.md), in the scope of the
project folder, with the rules, outputs and errors of the CLI. Design:
[ADR 0017](adr/0017-mcp-bridge.md).

## Declare it to Claude Code

```text
smf daemon install           # once per machine
smf daemon register          # in the project, once
smf mcp install              # in the project: adds the sermofur entry to .mcp.json
claude                       # approve the sermofur server when Claude Code asks
```

`smf mcp install` adds or updates only the `sermofur` entry of `.mcp.json` in the current folder
(or `--path`); every other server and the rest of the file stay byte for byte. Run again, it
writes nothing when the entry is current. A file that is not a JSON object, or whose
`mcpServers` is not one, fails with `invalid_mcp_config` and is left untouched. The command says
what remains to do (daemon, registration). The entry runs `smf mcp serve` when `smf` is on the
`PATH`; otherwise it names the full path of the executable, which is specific to the machine:
put `smf` on the `PATH` before committing `.mcp.json`. `smf mcp uninstall` removes the entry and
nothing else. `smf doctor` reports whether the instance root declares it (`mcp` check).

Claude Code asks you to approve a server declared in `.mcp.json` the first time it starts in the
project. The scope of the session is the project root, which Claude Code passes in
`CLAUDE_PROJECT_DIR`; other hosts use the folder they start the server in.

The user scope of Claude Code lives in `~/.claude.json`, which Claude Code rewrites constantly:
`smf` does not write it. To declare Sermofur for all your projects, run
`claude mcp add --scope user sermofur -- smf mcp serve`.

## Declare it to Codex

```text
smf mcp install --host codex                 # .codex/config.toml of the project
smf mcp install --host codex --scope user    # your Codex configuration, all projects
```

The command adds the `[mcp_servers.sermofur]` table, or updates only its `command` and `args`
lines: keys and sub-tables you add to it (an approval mode, `[mcp_servers.sermofur.env]`) stay.
Every other line, comments included, stays byte for byte, and the line endings of the file are
kept. Run again, it writes nothing when the table is current. The user
configuration is `~/.codex/config.toml`, or `config.toml` in `CODEX_HOME` when it is set.

Codex loads `.codex/config.toml` only in a project it trusts: start `codex` in the project once and
trust it. With the user scope, the scope of each session is the folder where Codex starts.

Codex asks for approval before each MCP tool call, and `codex exec` (approval `never`) refuses
them. Sermofur marks its four read tools as read-only and none of its tools as destructive: to let
Codex call the read tools without asking and still ask for writes, add
`default_tools_approval_mode = "writes"` to the `[mcp_servers.sermofur]` table (`"approve"` lets
every tool run), or pass `-c mcp_servers.sermofur.default_tools_approval_mode="writes"` to
`codex`.

There is no full TOML parser behind the edit: the command finds table headers outside strings and
refuses, with `invalid_mcp_config` and the file untouched, what it cannot change safely: a
multi-line string that is never closed, `sermofur` written as a key (`sermofur = { … }` in
`[mcp_servers]`, or a dotted `mcp_servers.sermofur…` key), or the table declared twice.
`smf mcp uninstall --host codex [--scope user]` removes the table and its sub-tables.

## Tools

Inputs are closed JSON objects (no extra field), strings of at most 16,384 characters without
NUL. No tool takes a path, a scope, an instance, an origin or an actor. Results are the JSON
output of the `smf` command, as structured content and as text; errors come as error results
`code: message`.

| Tool | Input | Runs |
|---|---|---|
| `sermofur_status` | none | `smf status` |
| `sermofur_context` | none | `status` and `scope current`: root, instance, scope, ancestors, counts |
| `sermofur_recall` | `question`, `limit` 1-3 | `smf recall` |
| `sermofur_challenge` | exactly one of `claimId`, `text` | `smf challenge`; writes nothing |
| `sermofur_claim` | `text`, `category` (episodic, semantic, procedural), `volatility`, `key` | `smf claim add` |
| `sermofur_evidence` | `claimId`, `kind`, `reference`, `lineage`, `contradicts`, `sourceId`, `key` | `smf evidence add` |
| `sermofur_record_retex` | `event`, `impact`, `next`, `key` | `smf retex add` |
| `sermofur_feedback` | `targetId`, `verdict` (helpful, not_applicable, wrong), `comment`, `key` | checks that the target is visible, then records a draft RETEX `feedback <verdict> on <kind> <id>` |

Every write carries the origin `llm` and an actor taken from the name the host declares (for
example `claude-code`, or the client name Codex gives): evidence declared by an LLM never reinforces a claim, a RETEX stays a draft,
and feedback changes neither confidence nor ranking. `preferences` and `decisions` claims need
the user's origin and are not offered. Values passed as CLI option values (`event`, `impact`,
`next`, `lineage`, `comment`, `key`, the `text` of a challenge) cannot start with `--`.

## Errors

| Code | What to do |
|---|---|
| `daemon_unavailable` | `smf daemon install` or `smf daemon start` |
| `daemon_version_mismatch` | `smf daemon restart`, then restart the server in the host |
| `not_served` | `smf daemon register` in the project |
| `no_instance` | `smf init` in the project |
| `invalid_input` | Input outside the schema, or a value starting with `--` |

Other codes are those of the CLI (`not_found`, `idempotency_conflict`…). The server stays up
whatever the state of the daemon and opens a new session when the daemon comes back. It writes
nothing to stdout outside the protocol; its journal (the daemon's) holds no argument and no
result.

## Limits

- Two hosts: Claude Code and Codex. Others can launch `smf mcp serve` by hand from their own
  configuration; `smf mcp install` does not write it.
- No resources, prompts or sampling: tools only.
- A host that keeps the server open across an update of `smf` gets `daemon_version_mismatch`
  after `smf daemon restart`, until it restarts the server.
