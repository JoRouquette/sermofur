[English](../mcp-integration.md) | Français

# Intégration MCP

`smf mcp serve` est un serveur MCP sur stdio pour Claude Code. Il n'a aucun moteur de mémoire
propre : chaque outil s'exécute comme une commande `smf` via le [daemon](daemon.md), dans le scope
du dossier du projet, avec les règles, les sorties et les erreurs de la CLI. Conception :
[ADR 0017](adr/0017-mcp-bridge.md). Codex vient ensuite.

## Le déclarer à Claude Code

```text
smf daemon install           # une fois par machine
smf daemon register          # dans le projet, une fois
smf mcp install              # dans le projet : ajoute l'entrée sermofur à .mcp.json
claude                       # approuver le serveur sermofur quand Claude Code le demande
```

`smf mcp install` n'ajoute ou ne met à jour que l'entrée `sermofur` du `.mcp.json` du dossier
courant (ou de `--path`) ; les autres serveurs et le reste du fichier restent identiques octet par
octet. Relancé, il n'écrit rien quand l'entrée est à jour. Un fichier qui n'est pas un objet JSON,
ou dont `mcpServers` n'en est pas un, échoue en `invalid_mcp_config` et n'est pas touché. La
commande dit ce qui reste à faire (daemon, enregistrement). L'entrée lance `smf mcp serve` quand
`smf` est dans le `PATH` ; sinon elle nomme le chemin complet de l'exécutable, propre à la
machine : mettre `smf` dans le `PATH` avant de commiter `.mcp.json`. `smf mcp uninstall` retire
l'entrée et rien d'autre. `smf doctor` signale si la racine de l'instance la déclare (contrôle
`mcp`).

Claude Code demande d'approuver un serveur déclaré dans `.mcp.json` la première fois qu'il démarre
dans le projet. Le scope de la session est la racine du projet, que Claude Code transmet dans
`CLAUDE_PROJECT_DIR` ; les autres hosts utilisent le dossier dans lequel ils démarrent le serveur.

## Outils

Les entrées sont des objets JSON fermés (aucun champ en plus), avec des chaînes d'au plus
16 384 caractères sans NUL. Aucun outil ne prend de chemin, de scope, d'instance, d'origine ni
d'acteur. Les résultats sont la sortie JSON de la commande `smf`, en contenu structuré et en
texte ; les erreurs arrivent comme résultats d'erreur `code: message`.

| Outil | Entrée | Exécute |
|---|---|---|
| `sermofur_status` | aucune | `smf status` |
| `sermofur_context` | aucune | `status` et `scope current` : racine, instance, scope, ancêtres, comptes |
| `sermofur_recall` | `question`, `limit` 1-3 | `smf recall` |
| `sermofur_challenge` | exactement un de `claimId`, `text` | `smf challenge` ; n'écrit rien |
| `sermofur_claim` | `text`, `category` (episodic, semantic, procedural), `volatility`, `key` | `smf claim add` |
| `sermofur_evidence` | `claimId`, `kind`, `reference`, `lineage`, `contradicts`, `sourceId`, `key` | `smf evidence add` |
| `sermofur_record_retex` | `event`, `impact`, `next`, `key` | `smf retex add` |
| `sermofur_feedback` | `targetId`, `verdict` (helpful, not_applicable, wrong), `comment`, `key` | vérifie que la cible est visible, puis enregistre un RETEX brouillon `feedback <verdict> on <kind> <id>` |

Toute écriture porte l'origine `llm` et un acteur tiré du nom que déclare le host (Claude Code :
`claude-code`) : une preuve déclarée par un LLM ne renforce jamais un claim, un RETEX reste
brouillon, et un feedback ne change ni la confiance ni le classement. Les claims `preferences` et
`decisions` exigent l'origine de l'utilisateur et ne sont pas proposés. Les valeurs passées comme
valeurs d'options de la CLI (`event`, `impact`, `next`, `lineage`, `comment`, `key`, le `text`
d'un challenge) ne peuvent pas commencer par `--`.

## Erreurs

| Code | Que faire |
|---|---|
| `daemon_unavailable` | `smf daemon install` ou `smf daemon start` |
| `daemon_version_mismatch` | `smf daemon restart`, puis redémarrer le serveur dans le host |
| `not_served` | `smf daemon register` dans le projet |
| `no_instance` | `smf init` dans le projet |
| `invalid_input` | Entrée hors du schéma, ou valeur qui commence par `--` |

Les autres codes sont ceux de la CLI (`not_found`, `idempotency_conflict`…). Le serveur reste en
marche quel que soit l'état du daemon et ouvre une nouvelle session quand le daemon revient. Il
n'écrit rien sur stdout hors du protocole ; son journal (celui du daemon) ne contient aucun
argument ni aucun résultat.

## Limites

- Un seul host pour l'instant : Claude Code. Codex est la prochaine spec du lot.
- Ni ressources, ni prompts, ni sampling : des outils seulement.
- Un host qui garde le serveur ouvert pendant une mise à jour de `smf` reçoit
  `daemon_version_mismatch` après `smf daemon restart`, jusqu'à ce qu'il redémarre le serveur.
