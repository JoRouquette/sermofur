English | [Français](../fr/adr/0003-ipc-daemon.md)

# ADR 0003-ipc-daemon — Shared daemon and local transport

Date: 2026-10-06. Status: accepted for design; the machine daemon is replaced by one daemon per
user ([ADR 0015](0015-user-daemon.md)), the transport is delivered in 0.4
([ADR 0016](0016-ipc-protocol.md)).

## Context
Sermofur mission: epistemic invariants, isolation and an installable local product.

## Decision
A machine daemon multiplexing registered instances, a CurrentUserOnly named pipe on Windows, a
0600 Unix domain socket on Unix. A stdio MCP bridge → daemon. Direct CLI only during bootstrap;
no second engine per host.

## Alternatives
Public HTTP and one engine per MCP client rejected. Loopback HTTP only if a host needs it, and
authenticated.

## Consequences
Boundary tests are required; changes go through a new ADR.
