English | [Français](../fr/adr/0001-storage.md)

# ADR 0001-storage — SQLite as authority, Markdown projections

Date: 2026-10-06. Status: accepted, implemented in 0.1.

## Context
Sermofur mission: epistemic invariants, isolation and an installable local product.

## Decision
Object, history and idempotency are atomic in SQLite; Markdown with JSON/YAML front matter can
be rebuilt after commit. DELETE journal mode during bootstrap, so that doctor sees no WAL
artifacts. A projection divergence is a diagnostic, never a competing truth. WAL comes with the
daemon, with a fixed native version and tests.

## Alternatives
Files as authority plus an index: needs an extra journal. Double write without a journal:
inconsistency.

## Consequences
Boundary tests are required; changes go through a new ADR. Completed by
[ADR 0010](0010-fail-closed-instance-discovery.md) (instance discovery).
