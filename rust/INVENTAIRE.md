# Inventaire de Sermofur .NET 0.4.3 (lot 0)

[validé par John le 2026-11-06] Relevé du 2026-11-06, en lecture seule, sur la base `d991c78` (code .NET identique à `v0.4.3`). Cinq relevés parallèles (CLI, domaine et magasin, instance et fichiers, démon, MCP et release), contrôlés par des comptages `grep` : 92 littéraux de code, 13 appels natifs, 10 variables d'environnement, 40 rubriques d'aide. Une affirmation sans référence `fichier:ligne` n'est pas établie. Le code fait foi sur la doc ; les écarts sont listés en section 10.

Références : chemins relatifs à `src/` sauf mention. `CR` = `Sermofur.Cli/CommandRunner.cs`, `CA` = `Sermofur.Cli/CommandArguments.cs`.

## 1. Commandes (27 formes locales, 10 `daemon`, 3 `mcp`)

Routage (`Sermofur.Cli/CliRouter.cs:90-138`) : `init`, `doctor`, `daemon *`, `mcp *`, aide, version et ligne vide restent locales. Toute autre commande passe par le démon s'il sert l'instance et si `SERMOFUR_NO_DAEMON` ne vaut pas `1`. Sortie JSON indentée même sans `--json`, sauf `root` et `doctor` (texte) (`CR:307-325`).

| Commande | Arguments et options | Sortie | Codes (exit) |
|---|---|---|---|
| `init` | aucun positionnel | `{root, configuration{schemaVersion, instanceId, createdAt}, created}` | invalid_path 1, nested_instance 4, invalid_instance 3, unsafe_path 4, foreign_owner 4, migration_required 3 |
| `root` | — | texte : chemin ; `--json` : `{root}` ; marche en format 1 | no_instance 2, invalid_instance 3, foreign_owner 4, unsafe_path 4 |
| `status` | — | `{root, configuration, scope, claims, evidence, retex, sources, mode, laya}` ; `mode` = `daemon` ou `direct`, `laya` = `unavailable` | no_instance 2, migration_required 3, storage_* 3 |
| `doctor` | — | texte ou `{root, overall, checks[{name, status, detail}]}` ; exit 0 ou 5 | unsafe_path 4 (lien) ; le reste est rapporté |
| `export` | — | `{files[]}` | storage_* 3, projection_pending 3 |
| `migrate` | — | `{migrated, from, to, backup}` | missing_database 3, instance_mismatch 3, storage_error 3, unsupported_schema 3 |
| `scope current` | — | `{scope, visible[]}` (tri ordinal) | — |
| `scope list`, `scope tree` | — (`tree` = `list`) | `[{id, kind, parentId, relativePath}]` | — |
| `scope add ID KIND PARENT PATH` | 4 positionnels | Scope | invalid_scope 1, duplicate_mapping 1, scope_boundary 4, invalid_path 1, unsafe_path 4 |
| `claim add TEXT` | `--origin` requis ; `--category`, `--volatility`, `--actor` (défaut `local-user`), `--key` | MemoryRecord `{id, scopeId, kind, status, revision, contentJson, provenance{origin, actor, timestamp}}` | invalid_input 1, user_choice_required 1, idempotency_conflict 1, projection_pending 3, storage_busy 3 |
| `claim list` / `show ID` | GUID non vide | liste / `{claim, confidence{level, reasons, independentOrigins}, evidence[], history[]}` | not_found 1 |
| `claim invalidate ID` | `--reason` requis, `--actor` | MemoryRecord ; idempotent | not_found 1, scope_boundary 4, invalid_input 1 |
| `evidence add CLAIM KIND REF` | `--lineage`, `--origin` requis ; `--contradicts`, `--source`, `--actor`, `--key` | MemoryRecord | not_found 1, scope_boundary 4, source_unavailable 1, idempotency_conflict 1 |
| `evidence list` / `show ID` | — | liste / MemoryRecord | not_found 1 |
| `retex add` | `--event`, `--impact`, `--next`, `--origin` requis ; `--actor`, `--key` | MemoryRecord (Draft) | invalid_input 1, idempotency_conflict 1 |
| `retex list` / `show ID` | — | liste / MemoryRecord | not_found 1 |
| `source add FILE` | `--origin` requis, `--actor` ; pas de `--key` | MemoryRecord (contentJson = SourceContent) | source_rejected 1, invalid_path 1, scope_boundary 4, unsafe_path 4 |
| `source list` / `show ID` | — | liste / `{source, history[]}` | not_found 1 |
| `source reindex [ID]` | `--actor` | `{sources[{id, path, outcome, previousHash, hash}]}` | not_found 1, storage_busy 3 |
| `index rebuild` | — | `{indexed, changedSources}` | storage_busy 3 |
| `recall QUESTION` | `--limit 1..3` (défaut 3) | `{query, results[], excluded}` | invalid_arguments 1, invalid_input 1, storage_busy 3 |
| `challenge ID` / `challenge --text T` | l'un ou l'autre | `{target, signals[], toConfront[]}` | not_found 1, invalid_arguments 1 |
| `daemon install`, `uninstall`, `start`, `stop`, `restart`, `status` | — | objet d'état `{state, manager, installed, running, answering, version, executable, pid, registry, startedAt, instancesOpen, clients}` | service_manager_unavailable 3, service_install_failed 3, daemon_unavailable 3 |
| `daemon register`, `unregister` | `--path` | `{root, registeredAt}` | not_registered 1, registry_busy 3, invalid_registry 3 |
| `daemon instances` | — | `[{root, registeredAt, missing}]` | invalid_registry 3 |
| `daemon run` | `--supervise` | ligne `listening` | daemon_already_running 3 |
| `mcp install`, `mcp uninstall` | `--path`, `--host claude-code\|codex`, `--scope project\|user` | `{file, changed, declared, command, portable, next[], host, replaced}` (JSON toujours) | invalid_arguments 1, invalid_mcp_config 3, unsafe_path 4 |
| `mcp serve` | — | protocole MCP sur stdout | — |

Pseudo-commandes : ligne vide (usage, 0) ; `-h` (rubrique, groupe ou usage, 0) ; `-v` (`Sermofur <v>` ou `{version}`, 0) ; `--json` seul (`invalid_arguments` 1) ; mot inconnu (`invalid_arguments` 1, mais `no_instance` 2 hors instance, car la découverte passe avant : `CR:151`, `CR:267-270`).

Aide : 40 rubriques dans `Sermofur.Cli/CommandHelp.cs:82-579`, une par forme ci-dessus ; groupes générés pour claim, evidence, retex, source, scope, index, daemon, mcp (`CommandHelp.cs:660-675`).

## 2. Parseur et codes de sortie

Parseur (fait en 1a) : `--json`, `--path`, `-h`, `-v`, `--`, 128 arguments, 16 384 unités UTF-16 sans NUL (`CA:25, 188-198`). Restent pour les lots suivants :
- Valeurs d'énumération : casse ignorée, `-` et `_` retirés (`CA:166-181`). `source-code`, `source_code` et `SourceCode` sont acceptés.
- GUID : `Guid.TryParse` (formats D, N, B, P, X acceptés) et non vide (`CA:183-186`) ; le MCP n'accepte que le format D (`Sermofur.Mcp/ToolInput.cs:150-153`).
- Drapeaux sans valeur : `json`, `contradicts`, `supervise` (`CA:12-17`).
- Commandes d'écriture (`CR:183-194`, `Sermofur.Cli/KnowledgeCommands.cs:15-21`) : `init`, `migrate`, `export`, `source add`, `source reindex`, `index *`, et toute autre commande dont le second mot est `add` ou `invalidate`. Le démon les sert une à la fois par instance.

Codes de sortie (`Sermofur.Domain/Models.cs:176`, exit 1 par défaut) :

| Exit | Codes |
|---|---|
| 1 | invalid_arguments, invalid_input, not_found, idempotency_conflict, user_choice_required, source_unavailable, source_rejected, invalid_path (exit 3 si le chemin de socket du démon est trop long, `DaemonPaths.cs:97-104`), invalid_scope, duplicate_mapping, wrong_kind (inatteignable en CLI), not_registered, request_too_large, response_too_large |
| 2 | no_instance |
| 3 | invalid_instance, migration_required, unsupported_schema, invalid_record, invalid_scope_tree, storage_busy (SQLite 5 et 6), storage_error, invalid_storage_or_path, projection_pending, missing_database, instance_mismatch, orphan_idempotency, invalid_registry, registry_busy, daemon_already_running, daemon_version_mismatch, daemon_interrupted, daemon_stopping, daemon_busy, daemon_unavailable, protocol_error, cli_only, not_served, service_manager_unavailable, service_install_failed, invalid_mcp_config |
| 4 | scope_boundary, unsafe_path, nested_instance, foreign_owner, foreign_endpoint |
| 5 | doctor `unhealthy` |

Erreur : `smf: <code>: <message>` sur stderr, ou `{"code","message"}` avec `--json` (`CR:104-126, 328-329`). Trois blocs de capture (`CR:104-126`) : `SermofurException` → son code et son exit ; `SqliteException` → `storage_busy` (codes 5 et 6) ou `storage_error`, message « Storage unavailable or inconsistent; run doctor. », exit 3 ; `JsonException`, `IOException`, `UnauthorizedAccessException`, `ArgumentException`, `InvalidOperationException` → `invalid_storage_or_path`, « Invalid path or data; run doctor. », exit 3. Toute autre exception n'est pas rattrapée : exit à relever sur l'oracle.

## 3. Domaine et confiance

Énumérations (`Sermofur.Domain/Models.cs:3-127`), forme JSON en snake_case, colonne SQL en PascalCase, `search.kind` en minuscules :
ScopeKind (workspace, client, project, repository, task) ; RecordKind (claim, evidence, retex, source) ; MemoryCategory (episodic, semantic, procedural, preferences, decisions) ; KnowledgeStatus (proposed, active, invalidated, superseded, draft) ; ActorKind (user, llm) ; Volatility (stable, evolving, volatile) ; EvidenceKind (execution, source_code, authoritative_documentation, project_decision, local_documentation, human_observation, user_assertion, llm_assertion) ; ConfidenceLevel (low, medium, high, verified) ; EvidenceRelation (supports, contradicts) ; SourceStatus (indexed, missing, unreadable, rejected) ; SourceRejection (too_large, binary, empty, unsafe_path) ; ReindexOutcome (unchanged, modified, missing, unreadable, rejected, restored, skipped ; défini dans `Sermofur.Application/Sources.cs:36-47`).

Portées (`Sermofur.Domain/ScopePolicy.cs`, `Sermofur.Application/ScopeService.cs`) : racine unique `workspace` ; parenté Client←Workspace, Project←Client, Repository←Project, Task←Repository ; cycles refusés (`invalid_scope_tree` 3) ; visibilité = contexte et ancêtres ; identifiant `[a-z][a-z0-9-]{0,63}` ; parent = contexte courant ; mapping stocké sous le nom trouvé sur le disque, avec `/` ; doublon et chevauchement de branches refusés ; un nouveau scope prend les sources d'ancêtre placées sous son mapping (`rescoped to {id}`, acteur `sermofur`).

Confiance (`Sermofur.Application/MemoryService.cs:170-268`), 15 règles :
- invalidé ou remplacé : low ; date de revue passée : low ;
- les preuves LLM ne renforcent jamais ; une origine = une `lineage` distincte ;
- soutien par origine : SourceCode et AuthoritativeDocumentation → high ; ProjectDecision, LocalDocumentation, HumanObservation → medium ; Execution, UserAssertion, LlmAssertion → low ;
- niveau = maximum ; contestation non LLM → plafond medium ; `verified` n'est jamais produit ;
- textes de raisons exacts aux lignes 175-265.

## 4. Magasin SQLite

- Schéma v2 fait en 1b (`Sermofur.Infrastructure/SqliteSchema.cs`). Connexion sans pool, FK actives, délai 5 s, écriture en transaction IMMEDIATE (`SqliteStore.cs:29-36, 189, 351, 521, 660, 743`).
- Historique : une entrée par création (`create`), invalidation (raison de l'appelant), changement de source (issue en minuscules), reclassement ; numéro AUTOINCREMENT, 0 dans le payload.
- Idempotence : clé `(scope_id, operation = kind PascalCase, key)` ; empreinte SHA-256 hexadécimale **majuscule** du JSON **indenté** `{scopeId, kind, contentJson, origin, actor}` (`SqliteStore.cs:522-533`). Les octets exacts comptent.
- Projection Markdown (`MarkdownProjection.cs:10-64`) : `.sermofur/records/<guid>.md`, front matter JSON indenté `schemaVersion: 1`, titre `# <Kind> <id>`, bloc ```json brut ; UTF-8 sans BOM ; écriture atomique non durable ; échec → `projection_pending` 3.
- Liste exhaustive des requêtes : `SqliteStore.cs` (24 opérations), `SqliteSearchIndex.cs`, `SqliteTokenizer.cs`.

## 5. Recherche, recall, challenge

- Tokenizer `unicode61 remove_diacritics 2` (`SqliteSchema.cs:11`), aussi pour la question (table temporaire `fts5vocab`, `SqliteTokenizer.cs:13-55`) ; 32 termes distincts au plus (comparaison ordinale), ordre de première apparition (`SearchTerms.cs:18-22`).
- BM25 calculé par Sermofur sur les passages visibles seulement (ADR 0013) : k1 = 1.2, b = 0.75, idf = ln(1 + (N − df + 0,5)/(df + 0,5)), longueur moyenne ≥ 1 (`RecallService.cs:70-71, 142-167`).
- Poids : low 1.0, medium 1.1, high et verified 1.2 ; arrêt anticipé ; ordre score desc, date desc, id asc ; non applicables en complément seulement ; score arrondi à 6 décimales par `Math.Round(x, 6)`, arrondi bancaire (au pair) par défaut (`RecallService.cs:181-253, 311, 330, 344`).
- Extrait : 300 unités UTF-16, fenêtre à 100 avant la première correspondance, `…` ; correspondance **InvariantCulture, IgnoreCase | IgnoreNonSpace** (`RecallService.cs:351-384`).
- Challenge : `not_applicable`, puis `review_due`, puis, pour chaque preuve du claim dans l'ordre de liste (par id), `contradiction` avant son signal de source (`source_unavailable` ou `source_changed`) : les signaux sont entrelacés par preuve, jamais triés par type (`ChallengeService.cs:40-86`) ; `toConfront` = 3 claims proches au plus ; n'écrit rien (`ChallengeService.cs`).

## 6. Sources, instance, migration, doctor

- Lecture de source (`Sermofur.Infrastructure/FileSourceReader.cs`) : 1 MiB, SHA-256 hexadécimal **minuscule** des octets lus (BOM compris), BOM retiré, UTF-8 strict, NUL → binaire, vide → rejet ; passages de 2 000 unités au plus, découpés sur `\n\n` (`Sermofur.Application/Sources.cs:64-138`).
- Découverte (`InstanceManager.cs:224-336`, `LocalPaths.cs:14-53`) : refus des chemins UNC et des lecteurs réseau ; refus d'un lien dans **tout** ancêtre du point de départ ; marqueur le plus proche, en échec fermé (Instance, Absent, Foreign, Damaged, Unreadable, ForeignOwner) ; `instance.json` ≤ 65 536 octets.
- `init` : staging `.sermofur-init-<guid32>`, renommage ; le perdant de la course relit le gagnant (`InstanceManager.cs:119-194`).
- Propriété (`FileOwnership.cs`) : Windows = SID propriétaire égal à l'utilisateur, ou Administrateurs si membre ; Linux `statx` sans suivre le lien ; macOS `lstat`.
- Chemins (`LocalPaths.cs:84-215`) : comparaison sans casse sous Windows, ordinale ailleurs (macOS compris) ; `CanonicalCase` (casse, NFC/NFD, alias 8.3 ou nom ambigu → `unsafe_path`).
- Écriture atomique (`AtomicFile.cs`) : temporaire `.<nom>.<12 car.>.tmp`, `Flush(durable)`, renommage ; liens refusés, ou suivis jusqu'à 10 pour les fichiers d'hôte MCP ; mode Unix conservé.
- Migration 1→2 (`InstanceMigration.cs`) : sauvegarde en ligne `backups/memory-v1-<yyyyMMddTHHmmssZ>-<guid32>.db`, transaction, reprise après interruption.
- Doctor (`InstanceDoctor.cs`), contrôles dans cet ordre : `instance`, `schema`, `storage`, `orphan_records`, `scopes`, `scope_mappings`, `scope_overlap`, `memory_consistency`, `projections`, `record_columns`, `fts5`, `search_index`, `source_scopes`, `sqlite`, puis `daemon`, `laya`, `model`, `mcp` ; `instance_storage` si une lecture échoue ; `overall` vaut `unhealthy` ou `healthy_with_warnings`, jamais `healthy`.

## 7. Démon

- Trame : longueur `uint32` little-endian, puis JSON UTF-8 ; requête ≤ 256 Kio, réponse ≤ 16 Mio ; champs nuls omis ; échappement `UnsafeRelaxedJsonEscaping` (≠ CLI) (`Sermofur.Daemon/Framing.cs`).
- Protocole 1, messages `hello`, `welcome`, `run`, `result`, `not_serving`, `status`, `shutdown`, `stopping`, `error`. La version de l'outil doit être identique (`Protocol.cs`, `DaemonServer.cs:212-428`).
- Emplacements (`DaemonPaths.cs:40-104`) :
  - Avec `SERMOFUR_DAEMON_HOME` (tous les tests .NET l'utilisent) : `<home>/config`, `<home>/state`, socket `<home>/run/daemon.sock`, ou tube `sermofur-home-<hash de la racine>` sous Windows.
  - Windows : `%APPDATA%/Sermofur`, `%LOCALAPPDATA%/Sermofur`, tube `sermofur-<16 hex du SHA-256 du SID>`, `CurrentUserOnly`.
  - Linux : `$XDG_CONFIG_HOME/sermofur` (défaut `~/.config`), `$XDG_STATE_HOME/sermofur` (défaut `~/.local/state`), socket `$XDG_RUNTIME_DIR/sermofur/daemon.sock`, sinon `$TMPDIR/sermofur-<uid>/daemon.sock` (`TMPDIR` par défaut `/tmp`).
  - macOS : `~/Library/Application Support/Sermofur`, `~/Library/Logs/Sermofur`, socket `$TMPDIR/sermofur-<uid>/daemon.sock`.
  - Unix : chemin de socket trop long → `invalid_path`, exit 3.
  - Unix : dossier 0700, socket 0600, identité du pair (`SO_PEERCRED`, `getpeereid`).
- Serveur : une écriture à la fois par instance (`FileTurn.cs`) ; lectures en parallèle ; échéance 60 s ; hello 10 s ; place de lecture 5 s ; une écriture commencée n'est jamais rejouée ; arrêt en drain ; deuxième signal = arrêt immédiat.
- Fichiers :
  - `daemon.lock` ;
  - `daemon.log` : trois fichiers de 1 Mio ;
  - `instances.json` : registre, 10 tentatives d'écriture ;
  - `service.json`.
- Services :
  - Windows : tâche planifiée `schtasks` avec superviseur (job object `KILL_ON_JOB_CLOSE`) ;
  - Linux : unité `systemd --user` ;
  - macOS : LaunchAgent.
  Variables enregistrées dans `service.json` : `DOTNET_ROOT`, `PATH`, `SERMOFUR_DAEMON_HOME`, chemins absolus seulement (`ServiceContracts.cs:26-88`). Le superviseur les ajoute sans vider l'environnement hérité (voir E3). Sous Windows, si `SERMOFUR_DAEMON_HOME` est porté (chemin absolu), l'installation échoue en `service_install_failed`, exit 3 ; une valeur relative est écartée avec un avertissement (`WindowsScheduledTask.cs:112-124`, `ServiceContracts.cs:127-136`, `DaemonCommands.cs:130-138`).

## 8. MCP

- Serveur stdio, nom `sermofur`, version = version produit, capacités `tools` seulement, SDK `ModelContextProtocol.Core` 2.2.0 (`Sermofur.Mcp/McpBridge.cs`).
- 8 outils (`Sermofur.Mcp/Tools.cs:82-229`) : `sermofur_status`, `sermofur_context`, `sermofur_recall`, `sermofur_challenge` (lecture seule), `sermofur_claim`, `sermofur_evidence`, `sermofur_record_retex`, `sermofur_feedback`. Chacun exécute 1 à 4 commandes `smf` par le démon ; les outils d'écriture seulement ajoutent `--origin llm --actor <client MCP normalisé>` (`Tools.cs:373-374`).
- Schéma : `additionalProperties: false`, bornes 1..16 384, `limit` 1..3, `format: uuid`, enum ; annotations `readOnlyHint`, `destructiveHint: false`, `openWorldHint: false`.
- Résultat : texte = JSON compact, `structuredContent` = objet (tableau enveloppé dans `{items}`) ; erreur : `isError`, `"<code>: <message>"`.
- Éditeurs de configuration à l'octet près :
  - `.mcp.json` : épissure JSON, 1 Mio au plus, commentaires refusés (`McpConfigFile.cs`) ;
  - TOML Codex : éditeur par lignes, CRLF conservé (`CodexConfigFile.cs`).
  Propriétés d'aller-retour testées avec FsCheck.

## 9. Appels natifs, environnement, release

Appels natifs (13 déclarations `DllImport`, 11 fonctions distinctes : `geteuid` est déclaré dans `FileOwnership.cs` et dans `UnixNative.cs`) :
- Windows, `kernel32` : `GetLongPathName` et 3 appels de job object.
- Linux, `libc.so.6` : `statx`, `geteuid`, `getsockopt(SO_PEERCRED)`.
- macOS, `libc` : `lstat`, `lstat$INODE64`, `geteuid`, `getpeereid`.
- APIs gérées Windows sans P/Invoke : SID propriétaire, ACL du tube.

Variables d'environnement :
- `SERMOFUR_NO_DAEMON`, `SERMOFUR_DAEMON_HOME` ;
- `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR`, `TMPDIR` ;
- `PATH`, `DOTNET_ROOT` (variables portées, `ServiceContracts.cs:26-31`), `CODEX_HOME`, `CLAUDE_PROJECT_DIR`.

Release :
- `.releaserc.json` publie `feat`, `fix`, `perf` et `revert` sur `main`.
- `ci.yml` construit sur 3 OS, puis enchaîne plan, release (approbation), package (attestation) et publish (environnement `nuget`, Trusted Publishing).
- `rust.yml` lance fmt, clippy et test sur 3 OS, sans publier.

## 10. Écarts relevés (code ≠ doc, ou défaut probable du .NET)

En cours d'arbitrage au comité du 2026-11-06 (E3, E4) ; les autres sont à décider par John lot par lot (règle : reproduire, sauf défaut avéré) :

| # | Constat | Référence | Proposition |
|---|---|---|---|
| E1 | `migrate` perdant de la course répond `migrated: true` (configuration lue avant le verrou) | `InstanceMigration.cs:63-65` | reproduire, consigner |
| E2 | ADR 0016 : une commande coupée en fin de drain est annulée ; le code ne l'annule pas, le processus sort | `DaemonServer.cs:493-498` | reproduire le code |
| E3 | Le superviseur laisse hériter tout l'environnement (`ProcessStartInfo.Environment` part d'une copie) ; la doc dit l'inverse | `Supervisor.cs:48-56` | défaut probable : à trancher |
| E4 | `daemon install` échoue si un démon d'une autre version répond et que la définition est identique | `DaemonCommands.cs:130-134` | défaut probable : à trancher |
| E5 | Deuxième trame pendant un `run` : `protocol_error`, l'écriture en cours continue sans réponse | `DaemonServer.cs:382-391` | reproduire (client non conforme seulement) |
| E6 | `stop` : systemd lève une erreur en cas d'échec, Windows et launchd l'ignorent | `SystemdUserService.cs:89` | reproduire |
| E7 | GUID vide : `invalid_input` attendu côté MCP, `invalid_arguments` obtenu via la CLI | `ToolInput.cs:150-153`, `CA:183-186` | reproduire |
| E8 | `.mcp.json` : une entrée `sermofur` modifiée perd les clés ajoutées par l'utilisateur ; Codex les garde | `McpConfigFile.cs:48-50` | reproduire |
| E9 | Doc : liens refusés sur `.sermofur` seulement ; code : sur tout ancêtre | `LocalPaths.cs:22, 39-53` | reproduire le code |
| E10 | Docs périmées : `verification.md:241` (canonicalisation), `cli.md:32` (« 0.1 »), `scope-model.md:31` (« lexical »), ADR 0005 (format 1), ADR 0017 (« une ou deux commandes ») ; mêmes textes ailleurs : `docs/fr/cli.md:34`, `docs/fr/verification.md:262`, `docs/fr/scope-model.md:32`, `docs/fr/adr/0005`, `docs/fr/adr/0017-mcp-bridge.md:17`, `docs/architecture.md:55`, `docs/fr/architecture.md:57` | — | hors portage |
| E11 | `PARITY.md` citait `docs/mcp.md`, qui n'existe pas : le fichier est `docs/mcp-integration.md` | — | corrigé |
| E12 | `schema::open` refuse un chemin vide (déjà listé dans `PARITY.md`) | — | décision de John |

## 11. Non-déterminisme à normaliser dans les fichiers d'or

- GUID ;
- horodatages : `UtcNow` de l'historique, `indexedAt`, `createdAt`, `startedAt`, format `O` dans les messages ;
- chemin de la racine ;
- `rowid` FTS ;
- pid ;
- version SQLite dans le contrôle `doctor/sqlite`.

Les dates de revue s'écrivent loin dans le futur ou dans le passé. La sortie de l'excerpt dépend de la culture (ICU sous Linux, NLS sous Windows) : les fichiers d'or s'enregistrent sous Windows.
