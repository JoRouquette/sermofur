English | [Français](../fr/adr/0008-synchronous-store.md)

# ADR 0008-synchronous-store — Synchronous IMemoryStore port in 0.1

Date: 2026-10-06. Status: accepted, implemented in 0.1.

## Context
The 0.1 vertical slice is a short-lived CLI process on local SQLite. Each command opens a
connection, runs a short immediate transaction and exits.

## Decision
The IMemoryStore port stays synchronous in 0.1: no network wait, no concurrent caller in the same
process, and an asynchronous API over Microsoft.Data.Sqlite would remain synchronous under the
hood. This choice is deliberate and limited to this version.

## Alternatives
An asynchronous port from 0.1: propagation cost with no measurable gain for a single-command CLI.

## Consequences
To be revisited with the daemon: a long-running process multiplexing instances and serving
CLI/MCP through IPC makes async and CancellationToken necessary. The revision will go through a
new ADR.
