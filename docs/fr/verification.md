[English](../verification.md) | Français

# Vérification

Comment vérifier soi-même une build de Sermofur, ce que couvrent les tests automatiques, et les
limites connues de la version 0.2.

## Vérifier soi-même

Depuis la racine du dépôt, avec le SDK .NET 10 (la CI passe les mêmes étapes sous Windows, Linux
et macOS pour chaque pull request) :

```powershell
dotnet tool restore                 # CSharpier, épinglé dans .config/dotnet-tools.json
dotnet restore --locked-mode        # échoue si une dépendance diffère des fichiers de verrouillage
dotnet build --no-restore           # attendu : 0 avertissement, 0 erreur (avertissements = erreurs)
dotnet test --no-restore            # attendu : tous les tests réussissent
dotnet csharpier check .            # attendu : aucun écart de formatage
```

Empaqueter et installer l'outil localement, puis l'exécuter :

```powershell
dotnet pack src/Sermofur.Cli -c Release -o artifacts/packages --no-restore
dotnet tool install Sermofur --version 0.0.0-dev --tool-path artifacts/tools --configfile nuget.local.config --no-cache
./artifacts/tools/smf --help      # attendu : exit 0, première ligne « Sermofur 0.0.0-dev »
```

L'essayer sur une instance jetable, jamais sur un dossier qui porte une vraie mémoire :

```powershell
mkdir $env:TEMP/sermofur-check; cd $env:TEMP/sermofur-check
<chemin-vers>/smf init --json     # "created": true
<chemin-vers>/smf claim add "sample claim" --origin user --json
<chemin-vers>/smf doctor          # Overall: healthy_with_warnings, exit 0
```

`healthy_with_warnings` est l'état attendu d'une instance saine : les capacités pas encore livrées
(laya, model) sont signalées en warning, de même que le daemon quand il ne tourne pas et le
serveur MCP quand la racine de l'instance ne le déclare pas.
`artifacts/` et `TestResults/` sont ignorés par Git.

## Ce que couvrent les tests

- Domaine : arbre de scopes, hiérarchie des types, visibilité des seuls ancêtres (arbres générés
  compris), cycles et parents invalides.
- Stockage : initialisation du schéma, clés étrangères (contrôle applicatif), persistance et
  historique entre connexions, idempotence (même clé, contenu changé, rejeux concurrents),
  schéma futur et base corrompue refusés, projection perdue ou impossible après commit puis
  reconstruite par export.
- Isolation : client A / client B, instances séparées, visibilité forgée refusée par le service,
  preuve liée au scope de son claim, erreurs inter-clients sans contenu.
- Scopes : clients imbriqués dans les deux sens, projet dans l'arbre d'un autre client (y compris
  par `..`), projets frères chevauchants, ID pris avec le même message quel que soit le
  propriétaire, mapping identique, slugs et types invalides, parent hors contexte, mappings avec
  NUL ou trop longs, `scope add` concurrent simulé juste avant la transaction, dossier mappé
  supprimé, mappings stockés avec des barres obliques, mappings stockés en `\` toujours lus, et
  mapping en `/` refusé comme doublon du même mapping stocké en `\`.
- Découverte (échec fermé, ADR 0010) : instance la plus proche, `init` imbriqué refusé, entrée
  `.sermofur` étrangère dans le dossier cible ou dans un parent qui arrête la découverte et
  `init` et reste intacte, instance enfant endommagée qui arrête la découverte (sans
  `instance.json`, `memory.db` seul, `records/` seul) et, par la CLI, `init`, `root`, `status`,
  `export`, `scope list` et `claim add` sans rien écrire dans l'instance ancêtre, entrée
  `.sermofur` illisible (ACE de refus, Windows seulement) qui arrête la découverte, lien
  `.sermofur` refusé, pendant ou non (une jonction sous Windows), y compris par `doctor`.
- doctor sur une entrée invalide : `invalid_instance: foreign`, `invalid_instance: damaged` et
  `invalid_instance: unreadable` dans le contrôle `instance`, exit 5, rien d'écrit dans
  l'instance ancêtre ; pour une entrée étrangère ou endommagée, rien d'écrit non plus dans
  l'entrée ni dans le dossier de départ quand doctor part d'un sous-dossier du dossier qui
  contient l'entrée.
- CLI : vrais processus et exécutions en processus, sortie JSON et codes de sortie, `--origin`
  obligatoire, `--help` n'importe où, séparateur `--`, bornes de la ligne de commande
  (128 arguments, 16 384 caractères, NUL), entrées aléatoires bornées sur l'identifiant de
  `claim show` (graine fixe), stdout UTF-8 avec texte non ASCII et stderr sans BOM (messages
  ASCII) dans un vrai processus, accents littéraux tandis que l'accent grave et le HTML restent
  échappés.
- doctor : lecture seule, `scope_overlap`, `scope_mappings`, identité incohérente, `search_index`
  désynchronisé, FTS5 disponible.
- Migration (ADR 0012) : une instance au format 1 construite par l'outil 0.1.1 publié (fixture de
  test) garde chaque enregistrement, ligne d'historique, clé d'idempotence, scope et projection ;
  claims et RETEX sont indexés ; la sauvegarde est une base au format 1 cohérente ; les autres
  commandes refusent le format 1 sans écrire ; une interruption après la sauvegarde, dans la
  transaction ou avant `instance.json` laisse une instance utilisable que `migrate` termine ;
  `migrate` est idempotent.
- Sources : empreinte des seuls octets lus, BOM accepté, fichier vide, binaire, Latin-1, trop gros,
  absent ou dossier refusé avec un code stable ; fichier d'un scope plus précis, d'un autre client,
  hors de l'instance ou dans `.sermofur` refusé ; ajout idempotent ; réindexation qui rend
  inchangée, modifiée (empreinte précédente dans l'historique), absente et restaurée, l'index
  suivant ; réindexation qui ne lit que les sources déclarées du scope courant ; preuve qui fige
  l'empreinte de sa source ; reconstruction après un index désynchronisé, qui relit chaque source avec historique sous
  l'acteur système, retire une source perdue et annule tout si une lecture échoue, et ne compte que
  le visible ; source d'un ancêtre déplacée vers un scope créé sur son dossier ; scope créé pendant
  un `source add` vu sous le verrou d'écriture ; un fichier, une source, quelle que soit la casse
  tapée, le système de fichiers décidant (casse et NFC/NFD) ; mapping de scope tapé dans une autre
  casse enregistré sous le nom du disque, avertissement de doctor sur une variante enregistrée ;
  `.sermofur` inatteignable par une variante de casse ; doctor qui signale une entrée d'index mal
  placée et une source restée dans un scope plus large ; fichier atteint par un lien refusé ; FIFO
  refusée sans blocage (Linux et macOS).
- Recall (ADR 0013) : au plus 3 résultats expliqués dans un ordre stable ; du contenu ajouté à un
  scope frère ne change ni la présence, ni l'ordre, ni le score des résultats visibles ; claims
  invalidés jamais en tête, comptés comme écartés ; meilleur passage et fraîcheur des sources ;
  syntaxe de requête traitée comme du texte ; bornes de la limite ; vrai processus sur une base en
  lecture seule sans rien écrire. Les termes de la question égalent ceux de l'index (accents, CJC,
  emoji, séparateurs).
- Challenge : contradiction indépendante qui plafonne la confiance à medium, contradiction d'un LLM
  notée sans réfuter, preuve du format 1 lue comme soutien, source modifiée puis disparue, claim
  invalidé et revue dépassée, au plus 3 claims proches jamais qualifiés de contradictions sans
  rien écrire, claim d'un autre client qui répond comme un inconnu.
- Propriétaire (ADR 0014) : entrée d'un autre compte refusée à la découverte et nommée par doctor,
  entrées propres reconnues, `instance.json` de plus de 64 Kio endommagé ; sous Linux et macOS,
  vraie entrée détenue par root via sudo sans mot de passe, propriétaire et groupe changés
  séparément (échoue si sudo n'est pas disponible) ; dans une session Windows élevée, entrée
  détenue par SYSTEM refusée et entrée détenue par Administrateurs acceptée.
- Propriétés (FsCheck) : la normalisation des mappings reste dans la racine, est stable et
  portable d'un séparateur à l'autre ; la grammaire de la ligne de commande garde positionnel tout
  argument après `--` et ne prend jamais une valeur d'option qui commence par `--`.
- Daemon (ADR 0015, 0016) : trames à 256 Kio et au-delà, tronquées, vides, JSON malformé et
  UTF-8 invalide, octets arbitraires (FsCheck), vecteurs d'arguments qui survivent à un aller-retour ;
  hello, version différente, second daemon, `run` dont le dossier diffère de celui de son `hello` ; registre
  (idempotence, mêmes refus qu'une commande directe, registre illisible ou trop gros laissé intact, instance
  déplacée), filtre de service (rien de lu hors du registre, instance imbriquée non servie,
  instances inactives et désenregistrées fermées) ; un vrai processus `smf daemon run` ; dossier du
  socket d'un autre compte (sudo) ou ouvert à d'autres refusé sous Linux et macOS ; treize vraies
  commandes comparées octet par octet avec et sans le daemon ; écritures passées par le daemon vues
  en direct ; `doctor` et `status` qui le signalent ; délais dépassés, départs pendant des écritures
  en file d'attente et en cours, rejeux par `--key` ; définitions de service (systemd, launchd, XML
  de la tâche et guillemets) ; commandes de service avec un gestionnaire simulé ; superviseur qui
  relance un daemon tué ; et, en CI seulement, le vrai service de chaque système installé, tué,
  piloté et retiré (`RealServiceTests`).
- L'ensemble des tests utilise son propre `SERMOFUR_DAEMON_HOME` : il n'atteint jamais le daemon
  du développeur.
- Pont MCP (ADR 0017) : le vrai protocole avec le client du SDK officiel contre un processus
  `smf mcp serve` (initialisation, huit outils aux schémas fermés, une écriture puis un recall
  identique à `smf recall`) ; outils de lecture identiques à la CLI ; claims d'un scope frère
  inconnus de challenge, evidence et feedback et absents du recall ; champs de chemin et de scope
  refusés ; écritures enregistrées en `llm` avec le host comme acteur, rejeux idempotents, textes
  en `--` conservés, valeurs d'option qui commencent par `--` refusées, `preferences` et
  `decisions` non proposés ; feedback enregistré comme RETEX brouillon qui ne change ni le rang ni
  la confiance ; pas de daemon (erreur en moins d'une seconde, serveur toujours en marche),
  instance non enregistrée, hors de toute instance, daemon d'une autre version, daemon redémarré
  sous le même pont, dix appels en parallèle ; `.mcp.json` modifié en gardant toute autre entrée
  identique octet par octet (FsCheck : installer puis retirer rend les mêmes octets), fichiers
  invalides laissés intacts, contrôle `mcp` de doctor.
- Déclaration à Codex : la table `[mcp_servers.sermofur]` ajoutée, ses lignes `command` et `args`
  mises à jour en gardant les clés et sous-tables ajoutées par l'utilisateur, la table retirée avec
  ses sous-tables, toute autre ligne gardée identique octet par octet, commentaires, profils, tableaux
  de tables, chaînes multilignes contenant une ligne qui ressemble à un en-tête et valeurs de
  tableaux laissés intacts, fichiers CRLF gardés en CRLF, chemins Windows écrits en chaînes
  littérales TOML ; refus (clé en ligne ou pointée, chaîne multiligne non fermée, table déclarée
  deux fois) ; FsCheck : installer puis retirer rend le même texte ; `--host`, `--scope`, la
  configuration utilisateur sous `CODEX_HOME`, doctor par host. Les tests utilisent leur propre
  `CODEX_HOME`.

## Mesures du daemon

`TenWritersNeverMeetStorageBusy` (`--filter Category=DaemonLoad`, une minute avec
`SERMOFUR_PERFORMANCE=1`) : le 2026-10-08, Windows 11, Intel Core i7-1255U, dix clients ont écrit
430 claims en dix secondes, sans perte ni doublon, sans `storage_busy` ; le daemon a ajouté
**2,9 ms** au 95e percentile à une commande sur une instance ouverte (limite de la spec : 50 ms).
Sans daemon, la CLI paie une recherche dans le système de fichiers avant de s'exécuter directement
(moins de 100 ms, `NoDaemonMeansNoClient`).

## Performance du recall

Mesure de référence de la spec (SC-004), `RecallPerformanceTests`, lancée par
`SERMOFUR_PERFORMANCE=1 dotnet test -c Release --filter Category=Performance` : 10 000 claims et
RETEX (la moitié des claims avec des preuves, dont certaines contraires) plus 1 000 sources
d'environ 20 Kio (11 000 objets, 20 000 passages indexés), 30 questions de trois termes fréquents,
chacune sur une base en lecture seule ouverte à nouveau, comme le fait une commande CLI. Le
2026-10-08, Windows 11, Intel Core i7-1255U, 32 Go : **p50 346 ms, p95 414 ms, max 443 ms**, sous
le plafond de 1 s de la spec. L'objectif de 300 ms au p95 fixé par le plan de la 0.1 n'est pas
atteint : l'essentiel du temps va à la lecture des fréquences de termes dans la table de
vocabulaire et des passages visibles.

## Limites connues

- Plateformes : la CI construit et lance les tests sous Windows, Linux et macOS pour chaque pull
  request et chaque push sur `main` ; les vérifications manuelles (page de code de la console,
  lecteur réseau) ont été faites sous Windows.
- Page de code de la console : la bascule en UTF-8 et la restauration de la page d'origine ne sont
  pas testées automatiquement, car les tests redirigent les deux flux et cette branche ne s'exécute
  alors pas. La restauration à la fin normale et sur erreur gérée a été vérifiée à la main sous
  Windows (cmd.exe, `chcp` avant et après). La restauration sur Ctrl+C n'a pas été vérifiée.
- Découverte : une entrée `.sermofur` ne compte comme instance que si c'est un dossier contenant
  `instance.json` ; toute autre entrée bloque Sermofur en dessous d'elle. Le cas illisible n'est
  testé que sous Windows. Les mappings sont comparés lexicalement (ni la casse, ni les alias 8.3,
  ni la normalisation Unicode ne sont canonisés). Le contrôle du propriétaire est vérifié sous
  Windows, Linux x64 et macOS arm64 ; macOS x64 ne l'est pas.
- Sous Unix, le renommage de publication d'`init` ne détecte pas un `.sermofur` vide créé à
  l'instant qui le précède ; cette course n'est pas testée.
- Un lecteur réseau mappé est refusé par le code, mais ce refus n'a pas été testé faute d'un tel
  lecteur.
- Non livré, donc non vérifié : inférence Laya, UI, installateur autonome, consolidation,
  similarité sémantique. Aucune coupure électrique simulée.
- La performance du recall (SC-004) est une mesure de référence, pas une garantie : voir [Performance du recall](#performance-du-recall).
  Elle ne tourne qu'avec `SERMOFUR_PERFORMANCE=1` et ne fait pas partie de la CI.
- Le paquet d'outil exige le runtime .NET 10.
