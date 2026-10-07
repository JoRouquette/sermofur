English | [Français](../fr/adr/0005-serialization.md)

# ADR 0005-serialization — Versioned, inspectable formats

Date: 2026-10-06. Status: accepted, implemented in 0.1.

## Context
Sermofur mission: epistemic invariants, isolation and an installable local product.

## Decision
`instance.json` version 1; canonical JSON objects in SQLite with a schema; Markdown with JSON
front matter that is valid YAML, for inspection. Manual changes to projections are never
re-imported implicitly. Explicit migration, backup and tests before future versions.

## Alternatives
An opaque format alone rejected; free YAML imported implicitly rejected.

## Consequences
Boundary tests are required; changes go through a new ADR.
