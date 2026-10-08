English | [Français](fr/instance-format.md)

# Instance format 2

An instance lives at the root of a working area (for example the folder that holds your
repositories). Another area has its own `.sermofur`, with a distinct identity and database. There
is no universal machine memory.

```text
.sermofur/
  instance.json       schemaVersion (2), instanceId, createdAt (UTC); at most 64 KiB
  memory.db           SQL schema 2: records (claims, evidence, RETEX, sources), history, scopes,
                      idempotency, metadata, full-text index search (FTS5)
  records/<GUID>.md   readable projections, JSON front matter that is valid YAML
  backups/            database backups made by smf migrate
```
A directory holds an instance only if it contains a `.sermofur` **directory** with `instance.json`.
Discovery walks up from the working directory and **stops at the nearest `.sermofur` entry, whatever
it is** ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)). Only a valid instance is used.
Any other entry stops every command except `doctor` with `invalid_instance` (exit 3) and a
message that names the case; `doctor` reports it in its `instance` check (exit 5):

- foreign entry: a file, or a folder that holds none of `instance.json`, `memory.db`, `records/`
  (empty folder, another tool's data) — rename or move it away;
- damaged instance: `memory.db` or `records/` without `instance.json` — restore `instance.json`;
- unreadable entry: its attributes or listing cannot be read (permissions), including when even
  its presence cannot be determined;
- entry of another user account: `foreign_owner` (exit 4), never read
  ([ADR 0014](adr/0014-instance-owner.md)).

A format 1 instance (created by 0.1) is refused with `migration_required` (exit 3) by every
command except `migrate`, `doctor` and `root`; `smf migrate` backs it up then migrates it
([ADR 0012](adr/0012-instance-format-2.md)).

A `.sermofur` link or junction, dangling or not, is refused with `unsafe_path` (exit 4) by every
command, `doctor` included. Discovery never goes on to an instance higher up: that would silently
write into another memory. `init` never adopts nor overwrites an entry: it stops with the same
error and leaves it untouched.

`init` builds the instance in an adjacent staging directory, then publishes it by a rename that
refuses an existing destination. On Unix, a concurrent empty `.sermofur` created in the instant
between that check and the rename is not detected. `init` never replaces an incomplete instance.
An interruption during staging may leave a `.sermofur-init-*` directory: do not publish nor
ingest it by hand. No existing data is cleaned up implicitly.

Scope mappings are stored relative to the instance root, **with forward slashes** (`a/b`)
whatever the operating system, so that an instance reads the same everywhere. Reading also
accepts `\`, so a mapping typed or stored with backslashes resolves identically.
A backslash is therefore always read as a separator, including in a directory name on Unix
([ADR 0009](adr/0009-portable-mapping-separator.md)).

Markdown is a projection, not a write source. Any manual change is reported as a divergence;
`export` restores the canonical version. Corrections go through the CLI and keep the history.
A source is a local text file declared with `source add`; its record holds the relative path,
the hash, the size and the indexing date, and its text lives only in the full-text index, which
`index rebuild` reconstructs from the registry and the files. A source projection shows the
metadata, never the content.
The working area should ignore `.sermofur` and the staging directories in Git, to avoid
synchronizing memory.
Backup: stop the writers then back up the whole `.sermofur`; never assume that a copy of the live
database is transactionally complete. `smf migrate` makes a consistent copy with the SQLite backup
API before changing the format.
