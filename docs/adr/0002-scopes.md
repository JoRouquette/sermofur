English | [Français](../fr/adr/0002-scopes.md)

# ADR 0002-scopes — Tree and isolation on every access

Date: 2026-10-06. Status: accepted, implemented in 0.1.

## Context
Sermofur mission: epistemic invariants, isolation and an installable local product.

## Decision
Workspace/client/project/repository/task; visibility of the context and its ancestors, not of
descendants or siblings. Explicit mappings, minimal scope; evidence relations stay within the
same scope in 0.1. Raw provenance never crosses a generic promotion.

## Alternatives
Prompt-only filtering rejected; workspace as super-user rejected.

## Consequences
Boundary tests are required; changes go through a new ADR.

## Adjustment found during bootstrap
A single-repository project may share the path of its project scope with its child repository;
no artificial folder is added. Sorting by length then by kind depth; no mapping is shared
between sibling scopes. Guarantee checked by a dedicated test.
0.1 hardening: a mapping can neither contain nor be contained in that of a scope outside its
lineage, rules carried by ScopeService (Application); a mapped directory that disappeared only
blocks its own context.
Superseded in part by [ADR 0009](0009-portable-mapping-separator.md) for the separator of stored
mappings.
