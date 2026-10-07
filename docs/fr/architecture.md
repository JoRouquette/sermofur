[English](../architecture.md) | Français

# Architecture

## Livré
Domain : types métier, arbre des scopes, hiérarchie des types et visibilité. Application :
MemoryService, ScopeService (toutes les règles d'enregistrement des scopes), validation, accès
scopés, confiance expliquée, port de stockage (`IMemoryStore`) et port de chemins
(`IPathResolver`). Infrastructure : découverte, chemins, bootstrap, SQLite, schéma, projections
et doctor. CLI : parsing, commandes et composition. Le port `IMemoryStore` est synchrone en 0.1
([ADR 0008](adr/0008-synchronous-store.md)).

```text
CLI → Application → Domain
  ↘ Infrastructure (implémente IMemoryStore, IPathResolver)
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
SQLite est l'unique autorité. Objet, historique et clé idempotente sont validés dans une
transaction immédiate courte ; clés étrangères actives ; attente bornée à 5 secondes. Journal
DELETE dans cette verticale. Markdown est écrit ensuite par renommage. Si la projection échoue,
`projection_pending` précise que l'objet est enregistré ; `export` la reconstruit. Une course de
projections peut laisser une ancienne révision : doctor la détecte, export la répare. Pas de
promesse de transaction atomique SQLite + fichiers. Un écrivain hostile sous le même utilisateur
n'est pas une frontière du système d'exploitation.

## Cible non livrée
Daemon machine multiplexant des instances séparées ; CLI, UI et pont MCP utilisent un IPC
local : pipe nommé CurrentUserOnly sous Windows, socket de domaine Unix 0600 sous Unix. Pont MCP
stdio sans moteur par host. Laya géré, paresseux, facultatif. Inspector Angular/Tauri exposant
preuves, conflits et historique. Aucune dépendance Anthropic/OpenAI dans Domain ou Application.
La planification vit dans un atelier Spec Kit hors dépôt ; voir [AGENTS.md](../../AGENTS.md).
