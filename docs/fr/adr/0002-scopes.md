[English](../../adr/0002-scopes.md) | Français

# ADR 0002-scopes — Arbre et isolation à tous les accès

Date : 2026-10-06. Statut : accepté, implémenté en 0.1.

## Contexte
Mission Sermofur : invariants épistémiques, isolation et produit installable local.

## Décision
Workspace/client/project/repository/task ; visibilité du contexte et de ses ancêtres, pas des
descendants ni des frères. Mappings explicites, scope minimal ; les relations de preuve restent
dans le même scope en 0.1. La provenance brute ne traverse jamais une promotion générique.

## Alternatives
Filtre par prompt seul rejeté ; workspace comme super-utilisateur rejeté.

## Conséquences
Tests de frontière requis ; toute évolution passe par un nouvel ADR.

## Ajustement constaté lors du bootstrap
Le projet mono-dépôt peut partager le chemin de son scope project avec son repository enfant ;
aucun dossier artificiel ajouté. Tri par longueur puis profondeur de type ; aucun partage de
mapping entre scopes frères. Garantie vérifiée par un test dédié.
Durcissement 0.1 : un mapping ne peut ni contenir ni être contenu dans celui d'un scope hors de
sa lignée, règles portées par ScopeService (Application) ; un dossier mappé disparu ne bloque que
son propre contexte.
Remplacé en partie par l'[ADR 0009](0009-portable-mapping-separator.md) pour le séparateur des
mappings stockés.
