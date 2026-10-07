[English](../../adr/0007-evidence.md) | Français

# ADR 0007-evidence — Support et autorité séparés

Date : 2026-10-06. Statut : accepté, implémenté en 0.1.

## Contexte
Mission Sermofur : invariants épistémiques, isolation et produit installable local.

## Décision
Une source utilisateur factuelle et un LLM restent low ; un choix utilisateur fait autorité sur
les préférences seulement. La lignée compte les origines indépendantes. Verified est réservé à une
preuve contrôlée, jamais auto-attestée par la CLI.

## Alternatives
Flottant arbitraire, vote d'agents et exécution déclarée comme preuve suffisante rejetés.

## Conséquences
Tests de frontière requis ; toute évolution passe par un nouvel ADR.
