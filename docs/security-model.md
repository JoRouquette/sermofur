English | [Français](fr/security-model.md)

# Security model

Local-first: no network listener, telemetry, cloud sync or sending to an LLM.
One OS user. Scopes protect the product's operations; the same OS user can access their SQLite
and Markdown files. No encryption, no multi-user ACL and no sandbox against hostile code.

Isolation in Application and filtered queries; IDs grant no permission; evidence stays in the
same scope. The service recomputes ancestors and does not trust a visibility forged by a caller.
Parameterized SQL; bounded inputs; unknown options refused; errors carry no business payload.
Evidence references are never opened. A source file is read only when the user adds or
reindexes it explicitly: inside the directory of the current scope and outside narrower scopes,
no link, junction, network path, special file or file of `.sermofur`, at most 1 MiB, read once.
Recall and challenge read no file. The full-text query is tokenized as text and sent as quoted
terms, so no query syntax reaches the engine, and ranking statistics come from visible scopes only
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
