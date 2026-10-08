[English](../../adr/0012-instance-format-2.md) | Français

# ADR 0012-instance-format-2 — Format d'instance 2 et migration explicite

Date : 2026-10-08. Statut : accepté, implémenté en 0.2. Complète
[l'ADR 0001](0001-storage.md).

## Contexte
Les sources, l'index plein texte et les preuves contraires demandent un nouveau stockage : un type
`Source` dans la table des enregistrements, dont SQLite ne sait pas modifier la contrainte CHECK
sur place, et une table FTS5. Les instances créées par la 0.1 doivent garder chaque objet, son
historique, ses clés d'idempotence et ses projections.

## Décision
- Format 2 : `instance.json` `schemaVersion: 2` et SQLite `user_version=2`. La table des
  enregistrements accepte `Source` et porte le chemin de la source, unique dans l'instance. La
  table FTS5 `search` (tokenizer `unicode61 remove_diacritics 2`) indexe claims, RETEX et passages
  de sources, écrits dans la même transaction que l'objet.
- La migration est explicite : `smf migrate`. Les autres commandes refusent le format 1 en
  `migration_required` (exit 3) ; `doctor` et `root` fonctionnent toujours. Aucune commande ne
  migre implicitement.
- `migrate` vérifie l'intégrité et les clés étrangères, sauvegarde la base par l'API de
  sauvegarde SQLite dans `.sermofur/backups/`, reconstruit la table des enregistrements et remplit
  l'index dans une seule transaction IMMEDIATE, revérifie les clés étrangères, valide, puis remplace
  `instance.json` par un fichier temporaire. Une base au format 2 avec un `instance.json` au
  format 1 est une migration inachevée que `migrate` termine.
- Les charges utiles des preuves ne sont pas réécrites : une charge du format 1 se lit comme une
  preuve qui soutient, sans source citée.

## Alternatives
Migration automatique à la première écriture : refusée par le mainteneur, une lecture pourrait
alors changer le format. Copier le fichier de base comme sauvegarde : incohérent tant qu'il est
ouvert. Une table `sources` séparée : historique, idempotence et projections seraient à refaire.

## Conséquences
Les utilisateurs de la 0.1 lancent `smf migrate` une fois ; la sauvegarde reste jusqu'à ce qu'ils
la suppriment. `backups/` rejoint les entrées que seul Sermofur crée dans `.sermofur`, pour la
classification de l'ADR 0010.
