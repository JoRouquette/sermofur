English | [Français](fr/architecture.md)

# Architecture

## Delivered
Domain: business types, the scope tree, the hierarchy of kinds and visibility. Application:
MemoryService, ScopeService (all scope registration rules), SourceService, RecallService (BM25 on
visible statistics), ChallengeService, validation, scoped access, explained confidence, and the
ports `IMemoryStore` (storage), `IPathResolver` (paths), `ISearchIndex` and `ITokenizer` (full-text
index), `ISourceReader` (source files). Infrastructure: discovery and owner check, paths,
bootstrap, SQLite, schema and migration, FTS5 index, file reader, projections and doctor. CLI:
parsing, commands and composition. The `IMemoryStore` port is synchronous
([ADR 0008](adr/0008-synchronous-store.md)).

```text
CLI → Application → Domain
  ↘ Infrastructure (implements IMemoryStore, ISearchIndex, ITokenizer, IPathResolver, ISourceReader)
```
`IMemoryStore.AddScope` receives a precondition supplied by ScopeService: the store opens its
immediate transaction, reads the scopes again and calls it before inserting. Duplicate and
overlap rules stay in Application; Infrastructure only guarantees that they apply to the state
read under the write lock. doctor reuses the same rule (`ScopeService.MappingConflicts`).
`IPathResolver.Relativize` gives the stored form of a mapping, with forward slashes on every
operating system.
The service recomputes visibility from the tree, without trusting a list of scopes supplied by
its caller. SQL reads are filtered before objects are loaded. Evidence stays in the scope of its
claim. Choosing an ID grants no additional right.

## Storage
SQLite is the single authority. Object, history, idempotency key and full-text index entries are
committed in one short immediate transaction (sources: `SaveSource` reads the current state and
applies the decision of SourceService under that transaction); foreign keys are enforced; waiting is bounded to 5 seconds. DELETE
journal mode in this vertical slice. Markdown is written afterwards by rename. If the projection
fails, `projection_pending` states that the object is saved; `export` rebuilds it. A race between
projections may leave an older revision: doctor detects it, export repairs it. There is no
promise of an atomic SQLite + files transaction. A hostile writer running as the same user is not
an OS boundary.

## Daemon
`Sermofur.Daemon` holds the local transport and the service: one daemon per user, installed as a
service of the session, serving the instances the user registered
([daemon.md](daemon.md), [ADR 0015](adr/0015-user-daemon.md)). The CLI sends its commands to the
daemon when it serves the instance; the daemon runs them with the same `CommandRunner`, given the
launch folder of the client, so both paths apply the same rules
([ADR 0016](adr/0016-ipc-protocol.md)). The daemon depends on no CLI type: the executor of
commands is handed to it by the CLI.

```text
smf ──→ CliRouter ──(no daemon / not served)──→ CommandRunner → Application
            └──(pipe / Unix socket)──→ DaemonServer ──→ CommandRunner → Application
```

## MCP bridge
`Sermofur.Mcp` holds `smf mcp serve`, an MCP server on stdio built with the official C# SDK, and
the editing of the host configurations (`.mcp.json` for Claude Code, TOML for Codex). Each tool becomes one or two `smf` commands sent to the daemon through
`DaemonClient`; the bridge has no engine ([ADR 0017](adr/0017-mcp-bridge.md)).

```text
Claude Code / Codex ──stdio──→ McpBridge ──(protocol 1)──→ DaemonServer ──→ CommandRunner → Application
```

## Target, not delivered
Laya managed, lazy and optional. An Angular/Tauri Inspector exposing
evidence, conflicts and history. No Anthropic/OpenAI dependency in Domain or Application.
Planning lives in a Spec Kit workshop outside the repository; see [AGENTS.md](../AGENTS.md).
