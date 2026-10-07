English | [Français](fr/security-model.md)

# Security model

Local-first: no network listener, telemetry, cloud sync or sending to an LLM in 0.1.
One OS user. Scopes protect the product's operations; the same OS user can access their SQLite
and Markdown files. No encryption, no multi-user ACL and no sandbox against hostile code.

Isolation in Application and filtered queries; IDs grant no permission; evidence stays in the
same scope. The service recomputes ancestors and does not trust a visibility forged by a caller.
Parameterized SQL; bounded inputs; unknown options refused; errors carry no business payload.
Declared sources are never opened: no traversal through an evidence reference.
Reparse points and UNC paths are refused for storage and mappings; init uses adjacent temporary
files and a rename. Any `.sermofur` entry that is not a valid instance (foreign,
damaged, unreadable, or a link) is never adopted nor overwritten and stops discovery, so that no
command ever writes into an ancestor instance
([ADR 0010](adr/0010-fail-closed-instance-discovery.md)).
Explicit limit: no protection against a concurrent path replacement by a malicious process of
the same user; consider resolution through handles before allowing links.

No technical log contains cognitive texts. stdout is an explicit output to the requester, to be
handled as private data. `.sermofur` must stay out of auto-synchronized Git.
doctor neither repairs nor migrates; it gives a global structural diagnosis without revealing
business payloads.
Blocking NuGet audit; pinned versions and committed lock files; the native SQLite runtime must be
checked before enabling WAL. Network sources and models only through a future explicit
installation.
