[English](../../adr/0017-mcp-bridge.md) | Français

# ADR 0017-mcp-bridge — Pont MCP client du daemon

Date : 2026-10-08. Statut : accepté, implémenté en 0.4. Fait suite à
[l'ADR 0003](0003-ipc-daemon.md), [l'ADR 0015](0015-user-daemon.md) et
[l'ADR 0016](0016-ipc-protocol.md).

## Contexte
Les noms d'outils et les règles du contrat MCP sont fixés depuis la 0.1 : pas de SQL, pas de
fichier arbitraire, pas de scope choisi par le modèle, des écritures sous forme de candidats
auditables. L'ADR 0003 interdit un moteur par host.

## Décision
- `smf mcp serve` est un serveur MCP sur stdio, construit avec le SDK C# officiel
  (`ModelContextProtocol.Core`, Apache-2.0, épinglé), sans le Generic Host de .NET.
- Chaque outil devient une ou deux commandes `smf` (toujours `--json`) que le daemon exécute par
  le protocole local 1, inchangé ; le résultat est la sortie JSON de la commande. Le pont n'ouvre
  jamais d'instance : pas de daemon, un daemon d'une autre version ou une instance non enregistrée
  donnent des résultats d'erreur qui nomment le remède, et le serveur reste en marche.
- Le scope est celui du dossier de lancement : `CLAUDE_PROJECT_DIR` quand le host la définit, le
  dossier courant sinon. Les schémas sont fermés et ne contiennent ni chemin, ni scope, ni
  instance, ni origine, ni acteur.
- C'est le canal, pas l'outil, qui fixe la provenance : origine `llm`, acteur normalisé à partir
  du nom du client donné à l'initialisation (`mcp-client` s'il est vide). C'est le « canal host »
  annoncé dans la documentation de la CLI.
- `sermofur_feedback` vérifie que sa cible est visible, puis enregistre un RETEX brouillon
  `feedback <verdict> on <kind> <id>` : aucun changement de format, aucun effet sur la confiance
  ni sur le classement ; le lot Reflect/Learn lira ces brouillons.
- `smf mcp install` modifie `.mcp.json` en remplaçant, en insérant ou en retirant les seuls octets
  de l'entrée `sermofur`, pour que les autres serveurs restent identiques octet par octet.

## Alternatives
De nouveaux messages IPC par outil (un second chemin vers le moteur, à garder identique). Un
moteur dans le pont (interdit par l'ADR 0003). Un nouveau type d'enregistrement pour le feedback
(format d'instance 3 et une migration pour chaque instance). `claude mcp add` (exige la CLI de
Claude Code et réécrit tout le fichier). Une implémentation maison du protocole (écartée).

## Conséquences
Le protocole est testé pour de vrai avec le client du SDK contre `smf mcp serve` sur les trois
systèmes. Le marqueur de feedback reste textuel jusqu'à ce que Reflect/Learn lui donne un modèle.
