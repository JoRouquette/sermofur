# Parité Rust / .NET 0.4.3

Statut : `todo`, `partial`, `done`. Une ligne ne passe à `done` que si ses scénarios d'or et ses tests de lecture croisée passent. Le détail sourcé de chaque ligne est dans `INVENTAIRE.md` (lot 0) ; la colonne « Tests .NET » nomme les fichiers de `tests/Sermofur.Tests/` à porter ou à rejouer en or.

| Lot | Contenu .NET (référence) | Crate Rust | Tests .NET | Statut |
|---|---|---|---|---|
| 0 | Inventaire : commandes, options, codes, outils MCP, écarts (`INVENTAIRE.md`) | n/a | n/a | done (validé par John le 2026-11-06) |
| 1a | Parseur, aide générale, version, bornes, JSON .NET | sermofur-core, smf | PropertyTests, CliTests (grammaire) | done (ea8f121) |
| 1b.0 | Formateurs .NET : `double` comme System.Text.Json (forme courte, exposant `E-07`), `DateTimeOffset` en JSON STJ, au format `O` et aux trois formats fixes, pour un instant et un décalage donnés ; lecture STJ ; table d'or produite par `rust/tools/format-oracle`. Hors lot : la lecture du décalage local courant (voir 7.2) | sermofur-core | n/a (or `formats.json`) | done |
| 1b.0b | Montée rusqlite 0.40.2 / libsqlite3-sys 0.38.2 (SQLite 3.53.2, la plus proche de 3.53.4 de l'oracle ; décision A6) ; rejouer l'or du schéma | sermofur-core | SearchIndexTests | done |
| 1b.1 | Schéma SQLite v2 : `create_database`, `validate` | sermofur-core | SearchIndexTests (FTS5) | done (3109a22) |
| 1b.2 | `instance.json` (lecture bornée 64 Kio, erreurs `invalid_instance`, `unsupported_schema`, `migration_required`), lecture STJ (BOM, doublons, casse, profondeur 64, GUID D) ; or `instance-config.json` de 111 cas produit par `rust/tools/config-oracle` ; `reject_links` sur tous les ancêtres | sermofur-core | InstanceTests, HardeningTests | done |
| 1b.3 | Découverte en échec fermé : `ValidateDirectory`, `RejectLinks` sur tous les ancêtres, six états du marqueur | sermofur-core | InstanceTests | todo |
| 1b.4 | `init` (staging, course entre processus), `root` | sermofur-core, smf | CliTests (init concurrent), InstanceTests | todo |
| 1c.1 | Propriété du dossier : Unix (`statx`, `lstat`, `geteuid`) | sermofur-core | HardeningTests | todo |
| 1c.2 | Propriété du dossier : Windows (SID propriétaire, Administrateurs), module dérogatoire `windows-sys` unique (décision A1 ; dérogation à inscrire avec l'accord de John) | sermofur-core | HardeningTests (ElevatedWindowsFact) | todo |
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
| 4.2 | `recall` : BM25 filtré, poids, ordre, complément, extrait (pliage NFD, retrait des marques, minuscules : décision A4), arrondi au pair | sermofur-core, smf | RecallTests, RecallPerformanceTests | todo |
| 4.3 | `challenge` : cinq signaux, `toConfront`, `--text` | sermofur-core, smf | ChallengeTests | todo |
| 5.1 | Lecture de source : 1 Mio, SHA-256, BOM, UTF-8 strict, rejets ; découpe en passages | sermofur-core | SourceTests | todo |
| 5.2 | `source add/list/show/reindex`, placement (`SourcePlacement`), reclassement par `scope add` | sermofur-core, smf | SourceTests | todo |
| 5.3 | `index rebuild` (une transaction, acteur `sermofur`) | sermofur-core, smf | SourceTests | todo |
| 6.1 | `migrate` 1→2 : sauvegarde en ligne, reprise, course | sermofur-core, smf | MigrationTests (fixture `instance-v1`) | todo |
| 6.2 | `doctor` : 18 contrôles dans l'ordre, sortie texte et JSON, exit 5 | sermofur-core, smf | DoctorTests, ScopeTests, SourceTests | todo |
| 7.1 | Trames, messages du protocole 1, limites | sermofur-daemon | FramingTests | todo |
| 7.2 | Chemins (`DaemonPaths`), verrou, journal tournant, registre `instances.json` ; décalage local courant (`DateTimeOffset.Now`) et choix de la dépendance de date `chrono` ou `time` après prototype (A5 ; report proposé à John le 2026-11-06, à valider) | sermofur-daemon | DaemonPathsTests, RegistryTests, SharedFilesTests | todo |
| 7.3 | Point de terminaison (tokio) : tube nommé `CurrentUserOnly` sur le SID Owner, comme l'oracle (Windows) ; socket 0600 et `peer_cred` (Unix) | sermofur-daemon | DaemonTests, HardeningTests | todo |
| 7.4 | Serveur (tokio, cœur synchrone par `spawn_blocking`) : session, tour d'écriture (le permis passe dans la tâche bloquante et reste suivi pour le drain : une échéance ne libère jamais le tour), échéances, drain, signaux ; `daemon run` | sermofur-daemon, smf | DaemonTests, DaemonSessionTests, DaemonLoadTests, DaemonRobustnessTests | todo |
| 7.5 | Client et routeur CLI : sonde, repli direct, relance des lectures, `SERMOFUR_NO_DAEMON` | sermofur-daemon, smf | RouterTests | todo |
| 7.6 | `daemon register/unregister/instances/status` | smf | DaemonTests, RegistryTests | todo |
| 7.7 | Services : `schtasks` et superviseur (job object via `win32job`, environnement hérité comme l'oracle : E3), `systemd --user`, LaunchAgent ; `daemon install/uninstall/start/stop/restart` (E4 corrigé) | sermofur-daemon, smf | ServiceTests, ServiceHardeningTests, RealServiceTests | todo |
| 8.1 | Éditeur `.mcp.json` à l'octet, propriétés d'aller-retour | sermofur-mcp | McpConfigFileTests | todo |
| 8.2 | Éditeur TOML Codex à l'octet, CRLF, propriétés | sermofur-mcp | CodexConfigFileTests | todo |
| 8.3 | `mcp install/uninstall` : `McpDeclaration`, lanceur portable, `next` | sermofur-mcp, smf | McpConfigFileTests, CodexConfigFileTests, ConfigurationWriteTests | todo |
| 8.4 | `mcp serve` : JSON-RPC 2.0 stdio écrit à la main (décision A3), 8 outils, schémas, annotations, session, canal démon | sermofur-mcp | McpTests, DaemonSessionTests | todo |
| 9 | Aide par commande : 40 rubriques et 8 groupes | smf | CliTests (EveryCommandHasItsOwnHelp, GroupHelp) | todo |
| 10 | Distribution : release, CI trois systèmes, installation | workflows | n/a | todo (décision de John) |

## Écarts assumés

Décision de John du 2026-11-06 (comité) : fidélité stricte. Les écarts E1 à E3 et E5 à E9 de `INVENTAIRE.md` section 10 se reproduisent ; l'ACL du tube garde le SID Owner.

- E4 corrigé : `daemon install` traite `daemon_version_mismatch` comme « pas à jour » et remplace le démon, comme `DaemonRuns` (`DaemonCommands.cs:218-223`), au lieu d'échouer (`DaemonCommands.cs:130-134`).
- Doubles (lot 1b.0), décision de John du 2026-11-06 : .NET 10.0.5 écrit 2^-25 (`2.980232238769531E-08`) et 2^-958 (`4.104536801298376E-289`) en un texte qui se relit comme le double précédent. Le Rust écrit la forme la plus courte qui se relit exactement. Inatteignable : seuls des scores arrondis à 6 décimales sont écrits. Le test d'or isole ces deux lignes et échoue sur une troisième.
- `schema::open` refuse un chemin de base vide (`invalid_path`, exit 1) ; le .NET ouvre une base temporaire. Jamais voulu, jamais atteint par la CLI (le chemin est toujours `<racine>/.sermofur/memory.db`). Décision de John à confirmer.

## Dérogations `unsafe`

(aucune ; propriété du dossier sous Windows en attente de décision de John)

## Dépendances

| Crate | Raison | Licence |
|---|---|---|
| rusqlite 0.40.2 (bundled, libsqlite3-sys 0.38.2) | SQLite 3.53.2 avec FTS5 et fts5vocab, la plus proche de la 3.53.4 de l'oracle (décision A6 du 2026-11-06) ; version verrouillée par le test `bundled_sqlite_is_the_version_chosen_for_parity` ; la montée ferme CVE-2025-7709 et CVE-2026-11822/11824 (FTS5) de la 3.50.2 précédente. Tire `rsqlite-vfs` 0.1.1 et `sqlite-wasm-rs` 0.5.5 pour la seule cible wasm32-unknown-unknown, non compilées ici : MIT, aucun avis RustSec au 2026-11-06, `rsqlite-vfs` sans dépôt déclaré | MIT |
| uuid | identité d'instance | MIT/Apache-2.0 |
| serde, serde_json, thiserror | sérialisation, erreurs | MIT/Apache-2.0 |
| proptest (dev) | propriétés portées de FsCheck | MIT/Apache-2.0 |
