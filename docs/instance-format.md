English | [Français](fr/instance-format.md)

# Instance format v1

An instance lives at the root of a working area (for example the folder that holds your
repositories). Another area has its own `.sermofur`, with a distinct identity and database. There
is no universal machine memory.

```text
.sermofur/
  instance.json       schemaVersion, instanceId, createdAt (UTC)
  memory.db           SQL schema v1: records, history, scopes, idempotency, metadata
  records/<GUID>.md   readable projections, JSON front matter that is valid YAML
```
A directory holds an instance only if it contains a `.sermofur` **directory** with `instance.json`.
Discovery walks up from the working directory and **stops at the nearest `.sermofur` entry, whatever
it is** ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)). Only a valid instance is used.
Any other entry stops every command with `invalid_instance` (exit 3), with a message that names
the case:

- foreign entry: a file, or a folder that holds none of `instance.json`, `memory.db`, `records/`
  (empty folder, another tool's data) — rename or move it away;
- damaged instance: `memory.db` or `records/` without `instance.json` — restore `instance.json`;
- unreadable entry: its attributes or content cannot be read (permissions), including when even
  its presence cannot be determined.

A `.sermofur` link or junction, dangling or not, is refused with `unsafe_path` (exit 4). Discovery
never goes on to an instance higher up: that would silently write into another memory. `doctor`
is the only command that runs on an invalid entry (except a link), to report it. `init` never adopts nor
overwrites an entry: it stops with the same error and leaves it untouched.

`init` builds the instance in an adjacent staging directory, then publishes it by an exclusive
rename; it never replaces an incomplete instance. An interruption during staging may leave a
`.sermofur-init-*` directory: do not publish nor ingest it by hand. No existing data is cleaned up
implicitly.

Scope mappings are stored relative to the instance root, **with forward slashes** (`a/b`)
whatever the operating system, so that an instance reads the same everywhere. Reading also
accepts `\`, so a mapping typed or stored with backslashes resolves identically.
A backslash is therefore always read as a separator, including in a directory name on Unix
([ADR 0009](adr/0009-portable-mapping-separator.md)).

Markdown is a projection, not a write source. Any manual change is reported as a divergence;
`export` restores the canonical version. Corrections go through the CLI and keep the history.
External sources are references only in this version.
The working area should ignore `.sermofur` and the staging directories in Git, to avoid
synchronizing memory.
Backup: stop the writers then back up the whole `.sermofur`, or use a future consistent SQLite
backup; never assume that a copy of the live database is transactionally complete.
