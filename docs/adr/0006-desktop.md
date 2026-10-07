English | [Français](../fr/adr/0006-desktop.md)

# ADR 0006-desktop — Angular and Tauri Inspector

Date: 2026-10-06. Status: accepted for design (future capabilities, not delivered).

## Context
Sermofur mission: epistemic invariants, isolation and an installable local product.

## Decision
An Angular/Tauri Inspector uses the daemon and the shared rules; no cognitive storage in the
frontend. The desktop shell waits for a working vertical slice and MCP.

## Alternatives
Electron not justified here; UI as source of truth rejected.

## Consequences
Boundary tests are required; changes go through a new ADR.
