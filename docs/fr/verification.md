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
(daemon, laya, model, mcp) sont signalées en warning.
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
  l'empreinte de sa source ; reconstruction après un index désynchronisé ; FIFO refusée (Linux et
  macOS).
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
  vraie entrée détenue par root via sudo sans mot de passe (échoue si sudo n'est pas disponible).
- Propriétés (FsCheck) : la normalisation des mappings reste dans la racine, est stable et
  portable d'un séparateur à l'autre ; la grammaire de la ligne de commande garde positionnel tout
  argument après `--` et ne prend jamais une valeur d'option qui commence par `--`.

## Performance du recall

Mesure de référence de la spec (SC-004), `RecallPerformanceTests`, lancée par
`SERMOFUR_PERFORMANCE=1 dotnet test -c Release --filter Category=Performance` : 10 000 claims et
RETEX plus 1 000 sources d'environ 20 Kio (11 000 objets, 20 000 passages indexés), 30 questions de
trois termes fréquents, base en lecture seule. Le 2026-10-08, Windows 11, Intel Core i7-1255U,
32 Go : **p50 329 ms, p95 411 ms, max 420 ms**, sous le plafond de 1 s de la spec. L'objectif de
300 ms au p95 fixé par le plan de la 0.1 n'est pas atteint : environ la moitié du temps va à la
lecture des fréquences de termes dans la table de vocabulaire, le reste aux passages visibles et
aux résumés des objets.

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
- Non livré, donc non vérifié : protocole MCP, inférence Laya, IPC du daemon, UI, installateur
  autonome, consolidation, similarité sémantique. Aucune coupure électrique simulée.
- La performance du recall (SC-004) est une mesure de référence, pas une garantie : voir plus bas.
  Elle ne tourne qu'avec `SERMOFUR_PERFORMANCE=1` et ne fait pas partie de la CI.
- Le paquet d'outil exige le runtime .NET 10.
