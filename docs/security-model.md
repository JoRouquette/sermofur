English | [Français](fr/security-model.md)

# Security model

Local-first: no network listener, telemetry, cloud sync or sending to an LLM.

The daemon ([daemon.md](daemon.md)) runs under your account and listens on no port: a named pipe
restricted to your account on Windows, a Unix socket `0600` in a folder `0700` elsewhere, whose
server checks the user ID of every peer. An endpoint or folder of another account, or one open to
others, is refused (`foreign_endpoint`) and the CLI runs directly. The daemon serves only the
instances you registered: a request for any other path is refused without reading it. The scope
of a session comes from the launch folder of the client and no request can change it. Requests
are bounded (256 KiB, 60 s); the journal records no argument and no output. Any process of your
account can use or stop your daemon, as it can already read your instance.

The MCP server ([mcp-integration.md](mcp-integration.md)) holds no engine: its tools run through
the daemon in the scope of the project folder. Its schemas are closed and take no path, scope,
instance, origin or actor; every write is recorded with the origin `llm` and the host as actor,
so a model cannot pass its statements off as yours, and an identifier of a sibling scope answers
like an unknown one. It offers no SQL, no file access, no resource and no prompt.

One OS user. Scopes protect the product's operations; the same OS user can access their SQLite
and Markdown files. No encryption, no multi-user ACL and no sandbox against hostile code.

Isolation in Application and filtered queries; IDs grant no permission; evidence stays in the
same scope. The service recomputes ancestors and does not trust a visibility forged by a caller.
Parameterized SQL; bounded inputs; unknown options refused; errors carry no business payload.
Evidence references are never opened. A source file is read only when the user adds or
reindexes it explicitly: inside the directory of the current scope and outside narrower scopes,
no link, junction, network path, special file or file of `.sermofur`, at most 1 MiB, read once.
Recall and challenge read no file. The type of a source is checked by path before it is
opened: a process that can write to the directory may swap the file in between, so sources must
live in folders only you can write to ([ADR 0014](adr/0014-instance-owner.md)). The full-text query is split into terms by the index tokenizer and
only compared with the index vocabulary, so no query syntax reaches the engine, and ranking statistics come from visible scopes only
([ADR 0013](adr/0013-filtered-ranking.md)).
Reparse points and UNC paths are refused for storage and mappings; init uses adjacent temporary
files and a rename. Any `.sermofur` entry that is not a valid instance (foreign, damaged,
unreadable, or a link) is never adopted nor overwritten and stops discovery, so that no command
ever writes into an ancestor instance ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)).
Explicit limits: no protection against a concurrent path replacement by a malicious process of
the same user (consider resolution through handles before allowing links). Discovery refuses a
`.sermofur` entry owned by another account (`foreign_owner`, [ADR 0014](adr/0014-instance-owner.md)),
so another user's entry can no longer receive your memory nor feed you theirs; it can still block
discovery and `init` below it (by default the root of a Windows drive, folders created at that
root, an NTFS data volume, FAT/exFAT media, or `/tmp`), and an instance you keep in a folder others
can write to can still be read or changed by them. On a shared machine, keep your instance in a
folder only you can write to and remove any entry another user created above it.

No technical log contains cognitive texts. stdout is an explicit output to the requester, to be
handled as private data. `.sermofur` must stay out of auto-synchronized Git.
doctor neither repairs nor migrates (`smf migrate` is explicit and backs up first); it gives a global structural diagnosis without revealing
business payloads.
Blocking NuGet audit; pinned versions and committed lock files; the native SQLite runtime must be
checked before enabling WAL. Network sources and models only through a future explicit
installation.
