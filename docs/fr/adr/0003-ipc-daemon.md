[English](../../adr/0003-ipc-daemon.md) | Français

# ADR 0003-ipc-daemon — Daemon partagé et transport local

Date : 2026-10-06. Statut : accepté pour conception (capacités futures non livrées).

## Contexte
Mission Sermofur : invariants épistémiques, isolation et produit installable local.

## Décision
Daemon machine multiplexant les instances enregistrées, pipe nommé CurrentUserOnly sous Windows,
socket de domaine Unix 0600 sous Unix. Pont MCP stdio → daemon. CLI directe seulement au
bootstrap ; pas de second moteur par host.

## Alternatives
HTTP public et moteur par client MCP rejetés. HTTP loopback seulement si un host le nécessite, et
authentifié.

## Conséquences
Tests de frontière requis ; toute évolution passe par un nouvel ADR.
