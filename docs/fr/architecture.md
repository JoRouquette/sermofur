[English](../architecture.md) | Français

# Architecture

## Livré
Domain : types métier, arbre des scopes, hiérarchie des types et visibilité. Application :
MemoryService, ScopeService (toutes les règles d'enregistrement des scopes), SourceService,
RecallService (BM25 sur statistiques visibles), ChallengeService, validation, accès scopés,
confiance expliquée, et les ports `IMemoryStore` (stockage), `IPathResolver` (chemins),
`ISearchIndex` et `ITokenizer` (index plein texte), `ISourceReader` (fichiers sources).
Infrastructure : découverte et contrôle du propriétaire, chemins, bootstrap, SQLite, schéma et
migration, index FTS5, lecteur de fichiers, projections et doctor. CLI : parsing, commandes et
composition. Le port `IMemoryStore` est synchrone
([ADR 0008](adr/0008-synchronous-store.md)).

```text
CLI → Application → Domain
  ↘ Infrastructure (implémente IMemoryStore, ISearchIndex, ITokenizer, IPathResolver, ISourceReader)
```
`IMemoryStore.AddScope` reçoit une précondition fournie par ScopeService : le stockage ouvre sa
transaction immédiate, relit les scopes et l'appelle avant l'insertion. Les règles de doublon et
de chevauchement restent en Application ; Infrastructure ne fait que garantir qu'elles portent
sur l'état lu sous verrou d'écriture. doctor réutilise la même règle
(`ScopeService.MappingConflicts`). `IPathResolver.Relativize` donne la forme stockée d'un
mapping, avec des barres obliques sur tous les systèmes.
Le service recalcule la visibilité depuis l'arbre, sans croire une liste de scopes fournie par
son appelant. Les lectures SQL sont filtrées avant de charger les objets. Les preuves restent au
scope de leur claim. Choisir un ID n'accorde aucun droit supplémentaire.

## Stockage
SQLite est l'unique autorité. Objet, historique, clé idempotente et entrées de l'index plein texte
sont validés dans une transaction immédiate courte (sources : `SaveSource` lit l'état courant et
applique la décision de SourceService sous cette transaction) ; clés étrangères actives ; attente bornée à 5 secondes. Journal
DELETE dans cette verticale. Markdown est écrit ensuite par renommage. Si la projection échoue,
`projection_pending` précise que l'objet est enregistré ; `export` la reconstruit. Une course de
projections peut laisser une ancienne révision : doctor la détecte, export la répare. Pas de
promesse de transaction atomique SQLite + fichiers. Un écrivain hostile sous le même utilisateur
n'est pas une frontière du système d'exploitation.

## Daemon
`Sermofur.Daemon` porte le transport local et le service : un daemon par utilisateur, installé
comme service de la session, qui sert les instances que l'utilisateur a enregistrées
([daemon.md](daemon.md), [ADR 0015](adr/0015-user-daemon.md)). La CLI envoie ses commandes au
daemon quand il sert l'instance ; le daemon les exécute avec le même `CommandRunner`, à partir du
dossier de lancement du client, si bien que les deux chemins appliquent les mêmes règles
([ADR 0016](adr/0016-ipc-protocol.md)). Le daemon ne dépend d'aucun type de la CLI : c'est la CLI
qui lui fournit l'exécuteur des commandes.

```text
smf ──→ CliRouter ──(pas de daemon / non servie)──→ CommandRunner → Application
            └──(pipe / socket Unix)──→ DaemonServer ──→ CommandRunner → Application
```

## Cible non livrée
Pont MCP stdio vers le daemon, sans moteur par host. Laya géré, paresseux, facultatif. Inspector
Angular/Tauri exposant preuves, conflits et historique. Aucune dépendance Anthropic/OpenAI dans Domain ou Application.
La planification vit dans un atelier Spec Kit hors dépôt ; voir [AGENTS.md](../../AGENTS.md).
