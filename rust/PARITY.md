# Parité Rust / .NET 0.4.3

Statut : `todo`, `partial`, `done`. Une ligne ne passe à `done` que si ses scénarios d'or et ses tests de lecture croisée passent. Le détail sourcé de chaque ligne est dans `INVENTAIRE.md` (lot 0) ; la colonne « Tests .NET » nomme les fichiers de `tests/Sermofur.Tests/` à porter ou à rejouer en or.

| Lot | Contenu .NET (référence) | Crate Rust | Tests .NET | Statut |
|---|---|---|---|---|
| 0 | Inventaire : commandes, options, codes, outils MCP, écarts (`INVENTAIRE.md`) | n/a | n/a | done (validé par John le 2026-11-06) |
| 1a | Parseur, aide générale, version, bornes, JSON .NET | sermofur-core, smf | PropertyTests, CliTests (grammaire) | done (ea8f121) |
| 1b.1 | Schéma SQLite v2 : `create_database`, `validate` | sermofur-core | SearchIndexTests (FTS5) | done (3109a22) |
| 1b.2 | `instance.json` (lecture bornée 64 Kio, erreurs `invalid_instance`, `unsupported_schema`, `migration_required`) | sermofur-core | InstanceTests, HardeningTests | todo |
| 1b.3 | Découverte en échec fermé : `ValidateDirectory`, `RejectLinks` sur tous les ancêtres, six états du marqueur | sermofur-core | InstanceTests | todo |
| 1b.4 | `init` (staging, course entre processus), `root` | sermofur-core, smf | CliTests (init concurrent), InstanceTests | todo |
| 1c.1 | Propriété du dossier : Unix (`statx`, `lstat`, `geteuid`) | sermofur-core | HardeningTests | todo |
| 1c.2 | Propriété du dossier : Windows (SID propriétaire, Administrateurs) | sermofur-core | HardeningTests (ElevatedWindowsFact) | todo (décision A1) |
| 1c.3 | `LocalPaths` : normalisation de mapping, `CanonicalCase` (casse, NFC/NFD, 8.3), UNC et lecteur réseau | sermofur-core | InstanceTests, SourceTests, PropertyTests | todo |
| 1c.4 | `AtomicFile` : temporaire, flush durable, renommage, liens refusés ou suivis (10), mode Unix | sermofur-core | ConfigurationWriteTests | todo |
| 2.1 | Énumérations, enregistrements, formes JSON (snake_case, camelCase) | sermofur-core | DomainTests | todo |
| 2.2 | `ScopePolicy` : racine, parenté, cycles, visibilité | sermofur-core | DomainTests (arbres générés) | todo |
| 2.3 | Confiance : 15 règles et textes de raisons | sermofur-core | DomainTests, ChallengeTests | todo |
| 3.1 | Magasin : création, lecture, liste, invalidation, historique, transactions IMMEDIATE, `storage_busy` | sermofur-core | MemoryTests, BoundaryTests | todo |
| 3.2 | Idempotence : clé, empreinte SHA-256 majuscule du JSON indenté, conflits, orpheline | sermofur-core | MemoryTests | todo |
| 3.3 | Projection Markdown, `projection_pending`, `export` | sermofur-core, smf | MemoryTests | todo |
| 3.4 | Commandes `status`, `scope current/list/tree/add`, `claim add/list/show/invalidate`, `evidence add/list/show`, `retex add/list/show` | smf | CliTests, ScopeTests, MemoryTests | todo |
| 3.5 | Bornes d'entrée, énumérations souples, GUID non vide, détection des écritures | smf | CliTests, BoundaryTests | todo |
| 4.1 | Tokenizer (table temporaire `fts5vocab`), index de recherche, termes de la question | sermofur-core | SearchIndexTests | todo |
| 4.2 | `recall` : BM25 filtré, poids, ordre, complément, extrait (culture), arrondi | sermofur-core, smf | RecallTests, RecallPerformanceTests | todo (décision A4) |
| 4.3 | `challenge` : cinq signaux, `toConfront`, `--text` | sermofur-core, smf | ChallengeTests | todo |
| 5.1 | Lecture de source : 1 Mio, SHA-256, BOM, UTF-8 strict, rejets ; découpe en passages | sermofur-core | SourceTests | todo |
| 5.2 | `source add/list/show/reindex`, placement (`SourcePlacement`), reclassement par `scope add` | sermofur-core, smf | SourceTests | todo |
| 5.3 | `index rebuild` (une transaction, acteur `sermofur`) | sermofur-core, smf | SourceTests | todo |
| 6.1 | `migrate` 1→2 : sauvegarde en ligne, reprise, course | sermofur-core, smf | MigrationTests (fixture `instance-v1`) | todo |
| 6.2 | `doctor` : 18 contrôles dans l'ordre, sortie texte et JSON, exit 5 | sermofur-core, smf | DoctorTests, ScopeTests, SourceTests | todo |
| 7.1 | Trames, messages du protocole 1, limites | sermofur-daemon | FramingTests | todo |
| 7.2 | Chemins (`DaemonPaths`), verrou, journal tournant, registre `instances.json` | sermofur-daemon | DaemonPathsTests, RegistryTests, SharedFilesTests | todo |
| 7.3 | Point de terminaison : tube nommé `CurrentUserOnly` (Windows), socket 0600 et identité du pair (Unix) | sermofur-daemon | DaemonTests, HardeningTests | todo (décisions A1, A2) |
| 7.4 | Serveur : session, tour d'écriture, échéances, drain, signaux ; `daemon run` | sermofur-daemon, smf | DaemonTests, DaemonSessionTests, DaemonLoadTests, DaemonRobustnessTests | todo |
| 7.5 | Client et routeur CLI : sonde, repli direct, relance des lectures, `SERMOFUR_NO_DAEMON` | sermofur-daemon, smf | RouterTests | todo |
| 7.6 | `daemon register/unregister/instances/status` | smf | DaemonTests, RegistryTests | todo |
| 7.7 | Services : `schtasks` et superviseur (job object), `systemd --user`, LaunchAgent ; `daemon install/uninstall/start/stop/restart` | sermofur-daemon, smf | ServiceTests, ServiceHardeningTests, RealServiceTests | todo (décisions A1, E3, E4) |
| 8.1 | Éditeur `.mcp.json` à l'octet, propriétés d'aller-retour | sermofur-mcp | McpConfigFileTests | todo |
| 8.2 | Éditeur TOML Codex à l'octet, CRLF, propriétés | sermofur-mcp | CodexConfigFileTests | todo |
| 8.3 | `mcp install/uninstall` : `McpDeclaration`, lanceur portable, `next` | sermofur-mcp, smf | McpConfigFileTests, CodexConfigFileTests, ConfigurationWriteTests | todo |
| 8.4 | `mcp serve` : serveur stdio, 8 outils, schémas, annotations, session, canal démon | sermofur-mcp | McpTests, DaemonSessionTests | todo (décision A3) |
| 9 | Aide par commande : 40 rubriques et 8 groupes | smf | CliTests (EveryCommandHasItsOwnHelp, GroupHelp) | todo |
| 10 | Distribution : release, CI trois systèmes, installation | workflows | n/a | todo (décision de John) |

## Écarts assumés

Écarts relevés au lot 0, en attente de décision : `INVENTAIRE.md` section 10 (E1 à E12).

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
