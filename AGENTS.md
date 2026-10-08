# Sermofur — instructions for coding agents

Specifications live in a Spec Kit workshop outside this repository; this repository carries no
`.specify` or `specs` folder and must not receive a copy of them. Durable decisions are recorded
as ADRs under `docs/adr/`; read the relevant ones before changing behavior. Never replace an
invariant by a prompt.

Architecture: Domain has no external dependency; Application carries the controls and the
ports (`IMemoryStore`, `IPathResolver`, `ISearchIndex`, `ISourceReader`, `ITokenizer`); Infrastructure implements the ports; the CLI composes.
No implicit network access at run time. Hosts never choose scopes: the scope comes from the
working directory.

Contracts that do not change without an ADR: error codes, exit codes, JSON field names, the
persisted format (SQLite schema, `instance.json`, Markdown projections).

Language: code, identifiers, comments, CLI messages and commits in English; documentation in
English (reference) and French (`README.fr.md`, `docs/fr/`), kept in sync.

Commands: `dotnet tool restore`, `dotnet restore --locked-mode`, `dotnet build`, `dotnet test`,
`dotnet csharpier format .`, `dotnet csharpier check .`. Commits follow Conventional Commits (they
decide the version, see `docs/release.md`) and are signed off (`git commit -s`, see
`CONTRIBUTING.md`).

Future capabilities not delivered: Laya, UI, learning and consolidation. The daemon
(`src/Sermofur.Daemon`, `docs/daemon.md`) and the MCP bridge for Claude Code and Codex
(`src/Sermofur.Mcp`, `docs/mcp-integration.md`) are delivered; tests isolate themselves from a real daemon
through `SERMOFUR_DAEMON_HOME`, and the real service test only runs in CI or with
`SERMOFUR_SERVICE_TESTS=1`.
