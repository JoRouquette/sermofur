[English](../../adr/0005-serialization.md) | Français

# ADR 0005-serialization — Formats versionnés inspectables

Date : 2026-10-06. Statut : accepté, implémenté en 0.1.

## Contexte
Mission Sermofur : invariants épistémiques, isolation et produit installable local.

## Décision
`instance.json` version 1 ; objets JSON canoniques en SQLite avec schéma ; Markdown à frontmatter
JSON valide YAML pour l'inspection. Les modifications manuelles des projections ne sont jamais
réimportées implicitement. Migration explicite, sauvegarde et tests avant les versions futures.

## Alternatives
Format opaque seul rejeté ; YAML libre importé implicitement rejeté.

## Conséquences
Tests de frontière requis ; toute évolution passe par un nouvel ADR.
