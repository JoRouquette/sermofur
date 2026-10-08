English | [Français](../fr/adr/0012-instance-format-2.md)

# ADR 0012-instance-format-2 — Instance format 2 and explicit migration

Date: 2026-10-08. Status: accepted, implemented in 0.2. Completes
[ADR 0001](0001-storage.md).

## Context
Sources, the full-text index and contradicting evidence need new storage: a `Source` kind in the
records table, whose CHECK constraint SQLite cannot alter in place, and an FTS5 table. Instances
created by 0.1 must keep every object, its history, its idempotency keys and its projections.

## Decision
- Format 2: `instance.json` `schemaVersion: 2` and SQLite `user_version=2`. The records table
  accepts `Source` and carries the source path, unique in the instance. The `search` FTS5 table
  (tokenizer `unicode61 remove_diacritics 2`) indexes claims, RETEX and source passages, written
  in the same transaction as the object.
- Migration is explicit: `smf migrate`. Other commands refuse format 1 with `migration_required`
  (exit 3); `doctor` and `root` still run. No command migrates implicitly.
- `migrate` checks integrity and foreign keys, backs the database up with the SQLite backup API
  to `.sermofur/backups/`, rebuilds the records table and fills the index in one IMMEDIATE
  transaction, checks foreign keys again, commits, then replaces `instance.json` through a
  temporary file. A database in format 2 with an `instance.json` in format 1 is an unfinished
  migration that `migrate` completes.
- Evidence payloads are not rewritten: a format 1 payload reads as supporting evidence that cites
  no source.
- Concurrency: the version is read again under the IMMEDIATE lock, so two concurrent `migrate`
  never both rebuild the database. The consistency check and the backup run just before that
  lock: no other version of `smf` must write to the instance while it migrates.

## Alternatives
Automatic migration on the first write: rejected by the maintainer, a read could then change the
format. Copying the database file as a backup: not consistent while it is open. A separate
`sources` table: history, idempotency and projections would have to be rebuilt for it.

## Consequences
Users of 0.1 run `smf migrate` once; the backup stays until they delete it. `backups/` joins the
entries that only Sermofur creates in `.sermofur`, for the classification of ADR 0010.
