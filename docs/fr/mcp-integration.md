[English](../mcp-integration.md) | Français

# Intégration MCP

`smf mcp serve` est un serveur MCP sur stdio pour Claude Code et Codex. Il n'a aucun moteur de
mémoire propre : chaque outil s'exécute comme une commande `smf` via le [daemon](daemon.md), dans
le scope du dossier du projet, avec les règles, les sorties et les erreurs de la CLI. Conception :
[ADR 0017](adr/0017-mcp-bridge.md).

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

Le scope utilisateur de Claude Code vit dans `~/.claude.json`, que Claude Code réécrit sans cesse :
`smf` ne l'écrit pas. Pour déclarer Sermofur dans tous vos projets, lancer
`claude mcp add --scope user sermofur -- smf mcp serve`.

## Le déclarer à Codex

```text
smf mcp install --host codex                 # .codex/config.toml du projet
smf mcp install --host codex --scope user    # votre configuration Codex, tous projets
```

La commande ajoute la table `[mcp_servers.sermofur]`, ou n'en met à jour que les lignes `command`
et `args` : les clés et sous-tables que vous y ajoutez (un mode d'approbation,
`[mcp_servers.sermofur.env]`) restent. Toutes les autres lignes, commentaires compris, restent
identiques octet par octet, et les fins de ligne du fichier sont conservées. Relancée, elle n'écrit
rien quand la table est à jour. La configuration utilisateur est `~/.codex/config.toml`, ou
`config.toml` dans `CODEX_HOME` quand il est défini.

Codex ne charge `.codex/config.toml` que dans un projet auquel il fait confiance : démarrer `codex`
une fois dans le projet et lui faire confiance. Avec le scope utilisateur, le scope de chaque
session est le dossier où Codex démarre.

Codex demande une approbation avant chaque appel d'outil MCP, et `codex exec` (approbation `never`)
les refuse. Sermofur marque ses quatre outils de lecture en lecture seule et aucun outil comme
destructeur : pour que Codex appelle les lectures sans demander et demande toujours pour les
écritures, ajouter `default_tools_approval_mode = "writes"` à la table `[mcp_servers.sermofur]`
(`"approve"` laisse passer tous les outils), ou passer
`-c mcp_servers.sermofur.default_tools_approval_mode="writes"` à `codex`.

Aucun parseur TOML complet n'est derrière la modification : la commande repère les en-têtes de
table hors des chaînes et refuse, en `invalid_mcp_config` et sans toucher au fichier, ce qu'elle ne
peut pas modifier sans risque : une chaîne multiligne jamais fermée, `sermofur` écrit comme clé
(`sermofur = { … }` dans `[mcp_servers]`, ou une clé pointée `mcp_servers.sermofur…`), ou la table
déclarée deux fois. `smf mcp uninstall --host codex [--scope user]` retire la table et ses
sous-tables ; les commentaires écrits juste au-dessus de la table suivante restent avec elle.
Installer puis désinstaller rend le fichier à l'octet près, à une exception : un fichier sans saut
de ligne final en reçoit un, dont l'en-tête de table a besoin, et le garde. Un nouveau fichier de
configuration est créé lisible par vous seul (Linux et macOS) ; un fichier existant garde ses
droits.

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

Toute écriture porte l'origine `llm` et un acteur tiré du nom que déclare le host (par exemple
`claude-code`, ou le nom de client que donne Codex) : une preuve déclarée par un LLM ne renforce
jamais un claim, un RETEX reste brouillon, et un feedback ne change ni la confiance ni le
classement. Les claims `preferences` et `decisions` exigent l'origine de l'utilisateur et ne sont
pas proposés. Les valeurs passées comme valeurs d'options de la CLI (`event`, `impact`, `next`,
`lineage`, `comment`, `key`, le `text` d'un challenge) ne peuvent pas commencer par `--`.

## Erreurs

| Code | Que faire |
|---|---|
| `daemon_unavailable` | `smf daemon install` ou `smf daemon start` |
| `daemon_version_mismatch` | Le message nomme le côté en retard : `smf daemon restart`, ou redémarrer le serveur dans le host |
| `daemon_interrupted` | Le daemon s'est arrêté pendant l'appel : vérifier si l'écriture a eu lieu avant de rappeler |
| `daemon_stopping` | Le daemon s'arrêtait ; rien n'a commencé, rappeler une fois qu'il est revenu |
| `not_served` | `smf daemon register` dans le projet |
| `no_instance` | `smf init`, puis `smf daemon register`, dans le projet |
| `invalid_input` | Entrée hors du schéma, ou valeur qui commence par `--` |
| `request_too_large` / `response_too_large` | Entrée de plus de 256 Kio, ou sortie de plus de 16 Mio |

Les autres codes sont ceux de la CLI (`not_found`, `idempotency_conflict`…). Le serveur reste en
marche quel que soit l'état du daemon et ouvre une nouvelle session quand le daemon revient ; un
appel n'est jamais envoyé deux fois. Un appel que le host annule ferme la session, que l'appel
suivant rouvre. Il
n'écrit rien sur stdout hors du protocole ; son journal (celui du daemon) ne contient aucun
argument ni aucun résultat.

## Limites

- Deux hosts : Claude Code et Codex. Les autres peuvent lancer `smf mcp serve` à la main depuis
  leur propre configuration ; `smf mcp install` ne l'écrit pas.
- Ni ressources, ni prompts, ni sampling : des outils seulement.
- Un host qui garde le serveur ouvert pendant une mise à jour de `smf` reçoit
  `daemon_version_mismatch` après `smf daemon restart`, jusqu'à ce qu'il redémarre le serveur.
