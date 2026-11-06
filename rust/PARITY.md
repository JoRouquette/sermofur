# Parité Rust / .NET 0.4.3

Statut : `todo`, `partial`, `done`. Une ligne ne passe à `done` que si ses scénarios d'or et ses tests de lecture croisée passent. Le premier lot du portage est l'inventaire : compléter cette table depuis `src/Sermofur.Cli/CommandRunner.cs`, `KnowledgeCommands.cs`, `docs/cli.md`, `docs/mcp.md` et les 17 ADR, avant tout autre code.

| Lot | Contenu .NET (référence) | Crate Rust | Statut |
|---|---|---|---|
| 0 | Inventaire exhaustif des commandes, options, codes d'erreur, outils MCP | n/a | todo |
| 1a | Parseur, aide, version, bornes, JSON .NET | sermofur-core, smf | done (ea8f121) |
| 1b | Schéma v2, `instance.json`, découverte, `init`, `root` | sermofur-core, smf | partial (schéma fait) |
| 1c | Propriété du dossier, liens, `unsafe_path`, `foreign_owner` | sermofur-core | todo |
| 2 | Domaine : portées, enregistrements (Claim, Evidence, Retex, Source), politique de portée | sermofur-core | todo |
| 3 | Magasin SQLite : écriture, historique, idempotence, projection Markdown | sermofur-core | todo |
| 4 | Recherche : FTS5, tokenizer, scoring, `recall`, `challenge` | sermofur-core | todo |
| 5 | Sources : `source add`, `source reindex`, `index` | sermofur-core | todo |
| 6 | `scope`, `migrate`, `doctor`, sauvegardes | sermofur-core, smf | todo |
| 7 | Démon : tube nommé (Windows), socket Unix, cycle de vie, `SERMOFUR_NO_DAEMON` | sermofur-daemon | todo |
| 8 | Serveur MCP (stdio), outils, schémas | sermofur-mcp | todo |
| 9 | Aide par commande, textes, documentation générée | smf | todo |
| 10 | Distribution : release, CI trois systèmes, instructions d'installation | workflows | todo |

## Écarts assumés

- `schema::open` refuse un chemin de base vide (`invalid_path`, exit 1) ; le .NET ouvre une base temporaire. Jamais voulu, jamais atteint par la CLI (le chemin est toujours `<racine>/.sermofur/memory.db`). Décision de John à confirmer.

## Dérogations `unsafe`

(aucune ; propriété du dossier sous Windows en attente de décision de John)

## Dépendances

| Crate | Raison | Licence |
|---|---|---|
| rusqlite (bundled) | SQLite avec FTS5 et fts5vocab, même moteur que le .NET | MIT |
| uuid | identité d'instance | MIT/Apache-2.0 |
| serde, serde_json, thiserror | sérialisation, erreurs | MIT/Apache-2.0 |
| proptest (dev) | propriétés portées de FsCheck | MIT/Apache-2.0 |
