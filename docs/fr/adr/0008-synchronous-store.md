[English](../../adr/0008-synchronous-store.md) | Français

# ADR 0008-synchronous-store — Port IMemoryStore synchrone en 0.1

Date : 2026-10-06. Statut : accepté, implémenté en 0.1.

## Contexte
La verticale 0.1 est une CLI à processus court sur SQLite local. Chaque commande ouvre une
connexion, exécute une transaction immédiate courte et se termine.

## Décision
Le port IMemoryStore reste synchrone en 0.1 : aucune attente réseau, aucun appelant concurrent
dans le même processus, et une API asynchrone sur Microsoft.Data.Sqlite resterait synchrone sous
le capot. Ce choix est assumé et borné à cette version.

## Alternatives
Port asynchrone dès 0.1 : coût de propagation sans gain mesurable pour une CLI mono-commande.

## Conséquences
À réviser avec le daemon : un processus long multiplexant les instances et servant CLI/MCP par
IPC rend l'asynchrone et le CancellationToken nécessaires. La révision passera par un nouvel ADR.
