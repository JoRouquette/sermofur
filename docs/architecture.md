English | [Français](fr/architecture.md)

# Architecture

## Delivered
Domain: business types, the scope tree, the hierarchy of kinds and visibility. Application:
MemoryService, ScopeService (all scope registration rules), validation, scoped access,
explained confidence, storage port (`IMemoryStore`) and path port (`IPathResolver`).
Infrastructure: discovery, paths, bootstrap, SQLite, schema, projections and doctor. CLI:
parsing, commands and composition. The `IMemoryStore` port is synchronous in 0.1
([ADR 0008](adr/0008-synchronous-store.md)).

```text
CLI → Application → Domain
  ↘ Infrastructure (implements IMemoryStore, IPathResolver)
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
SQLite is the single authority. Object, history and idempotency key are committed in one short
immediate transaction; foreign keys are enforced; waiting is bounded to 5 seconds. DELETE
journal mode in this vertical slice. Markdown is written afterwards by rename. If the projection
fails, `projection_pending` states that the object is saved; `export` rebuilds it. A race between
projections may leave an older revision: doctor detects it, export repairs it. There is no
promise of an atomic SQLite + files transaction. A hostile writer running as the same user is not
an OS boundary.

## Target, not delivered
A machine daemon multiplexing separate instances; CLI, UI and MCP bridge use local IPC: a
CurrentUserOnly named pipe on Windows, a 0600 Unix domain socket on Unix. A stdio MCP bridge with
no engine per host. Laya managed, lazy and optional. An Angular/Tauri Inspector exposing
evidence, conflicts and history. No Anthropic/OpenAI dependency in Domain or Application.
Planning lives in a Spec Kit workshop outside the repository; see [AGENTS.md](../AGENTS.md).
