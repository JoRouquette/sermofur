[English](../../adr/0001-storage.md) | Français

# ADR 0001-storage — SQLite autorité et projections Markdown

Date : 2026-10-06. Statut : accepté, implémenté en 0.1.

## Contexte
Mission Sermofur : invariants épistémiques, isolation et produit installable local.

## Décision
Objet, historique et idempotence atomiques en SQLite ; Markdown à frontmatter JSON/YAML
reconstructible après commit. Journal DELETE au bootstrap, pour que doctor ne voie pas
d'artefacts WAL. Une divergence de projection est un diagnostic, jamais une vérité concurrente.
WAL arrivera avec le daemon, avec une version native corrigée et des tests.

## Alternatives
Fichiers autorité + index : journal supplémentaire nécessaire ; double écriture sans journal :
incohérence.

## Conséquences
Tests de frontière requis ; toute évolution passe par un nouvel ADR.
