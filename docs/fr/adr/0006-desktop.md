[English](../../adr/0006-desktop.md) | Français

# ADR 0006-desktop — Inspector Angular et Tauri

Date : 2026-10-06. Statut : accepté pour conception (capacités futures non livrées).

## Contexte
Mission Sermofur : invariants épistémiques, isolation et produit installable local.

## Décision
Un Inspector Angular/Tauri utilise le daemon et les règles communes ; aucun stockage cognitif dans
le frontend. Le shell desktop attend une verticale et un MCP fonctionnels.

## Alternatives
Electron non justifié ici ; UI source de vérité rejetée.

## Conséquences
Tests de frontière requis ; toute évolution passe par un nouvel ADR.
