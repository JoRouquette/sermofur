English | [Français](../fr/adr/0007-evidence.md)

# ADR 0007-evidence — Support and authority kept apart

Date: 2026-10-06. Status: accepted, implemented in 0.1.

## Context
Sermofur mission: epistemic invariants, isolation and an installable local product.

## Decision
A factual user source and an LLM stay low; a user choice is authoritative on preferences only.
Lineage counts independent origins. Verified is reserved for controlled evidence, never
self-attested through the CLI.

## Alternatives
An arbitrary float, agent voting and declared execution as sufficient evidence rejected.

## Consequences
Boundary tests are required; changes go through a new ADR.
