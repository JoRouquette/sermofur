[English](../daemon.md) | Français

# Daemon

Le daemon garde vos instances ouvertes pour tous les clients de votre session : la CLI
aujourd'hui, le pont MCP ensuite. Il tourne sous votre compte, comme service de votre session, et
ne sert que les instances que vous enregistrez. Sans lui, `smf` fonctionne exactement comme
avant. Conception : [ADR 0015](adr/0015-user-daemon.md), [ADR 0016](adr/0016-ipc-protocol.md).

## Installer

```text
smf daemon install
smf daemon register          # depuis une instance, ou avec --path
smf status                   # "mode": "daemon"
```

`install` ne demande aucun droit d'administrateur. Il enregistre un service auprès du
gestionnaire de votre session, le démarre et attend qu'il réponde :

| Système | Service | Relance après un plantage |
|---|---|---|
| Windows | Tâche planifiée `\Sermofur\Daemon`, à l'ouverture de session, jeton interactif | en moins d'une seconde (superviseur) |
| Linux | `~/.config/systemd/user/sermofur.service` | après une seconde |
| macOS | `~/Library/LaunchAgents/io.github.jorouquette.sermofur.plist` | après une seconde |

Relancer `install` ne change rien quand la même version est installée ; une autre version ou un
autre exécutable `smf` remplace le service. Là où aucun gestionnaire de services n'existe
(conteneur, WSL sans systemd), `install` échoue en `service_manager_unavailable` : lancer plutôt
`smf daemon run` dans un terminal, ou garder la CLI directe.

## Utiliser

Une fois une instance enregistrée, toute commande `smf` lancée à l'intérieur passe par le daemon,
avec la même sortie, les mêmes erreurs et les mêmes codes de sortie. Le daemon est le seul
écrivain de l'instance : des commandes concurrentes n'attendent plus et n'échouent plus en
`storage_busy`.

Celles-ci s'exécutent toujours dans la CLI elle-même : `smf daemon …`, `smf mcp …`, `init`,
`doctor`, `--help`, `--version` ; le daemon les refuse, quel que soit le client (`cli_only`). Les
commandes pour une instance non enregistrée s'exécutent directement.

Si le daemon s'arrête pendant une commande (plantage, tâche Windows arrêtée hors de `smf`, ou
commande encore en cours à la fin du délai d'arrêt), une lecture est relancée directement ; une
écriture n'est jamais exécutée deux fois : la CLI signale `daemon_interrupted`, et vous vérifiez
son résultat avant de la relancer. Un arrêt laisse les commandes déjà commencées se terminer et
répondre (65 s au plus) ; une commande qui attendait encore son tour ou une place reçoit
`daemon_stopping` et n'a pas commencé (la CLI relance directement une lecture). Pendant ce temps, les nouvelles commandes ne trouvent aussitôt aucun daemon et
s'exécutent directement : sous Linux et macOS le socket est supprimé ; sous Windows le pipe reste
listé mais ferme toute nouvelle connexion. `smf daemon stop`, `restart`, `install` et `uninstall`
attendent la fin du daemon (75 s au plus) avant d'appeler le gestionnaire de services, et systemd
comme launchd lui laissent autant avant de le forcer. Au bout d'une seconde d'attente, elles le
signalent sur stderr, et elles avertissent quand le délai est écoulé ; avec `--json`, stderr ne
garde que l'erreur JSON. Un daemon qui vient de démarrer et n'écoute pas encore reçoit de nouveau
la demande d'arrêt jusqu'à ce qu'il réponde.

Sous charge, le daemon exécute au plus une commande par cœur du processeur moins un (deux au
moins), chacune sur un fil qui lui est propre ; les connexions, le statut et l'arrêt n'attendent
jamais de place. Une commande dispose de 60 s à partir de son arrivée au daemon : si son tour
(une écriture derrière d'autres écritures sur la même instance) ou sa place n'est pas venu d'ici
là, ou vient alors qu'il reste moins de 3 s, elle reçoit `daemon_busy` et n'a pas commencé. Une
lecture attend une place 5 s au plus, puis reçoit aussi `daemon_busy` ; la CLI relance
directement une lecture. La CLI attend la réponse 70 s ; au-delà, elle considère que
le daemon s'est arrêté pendant la commande : une lecture est relancée directement, une écriture
signale `daemon_interrupted` (le daemon peut encore l'exécuter). `smf daemon status` laisse 5 s au
daemon pour répondre, puis le donne `running` avec `"answering": false` ; `smf daemon restart` ou
`smf daemon install` remplace un tel daemon. Le journal ne retient jamais une commande : les lignes
sont mises en file et écrites en arrière-plan.

| Commande | Effet |
|---|---|
| `smf daemon status [--json]` | `absent`, `installed_stopped`, `running`, `version_mismatch`, `foreign_endpoint`, `service_manager_unavailable` ; si un daemon en marche répond, version, processus, démarrage, instances ouvertes, clients |
| `smf daemon start` / `stop` / `restart` | Pilote le service installé |
| `smf daemon register` / `unregister` | Ajoute ou retire l'instance trouvée depuis le dossier (ou `--path`) |
| `smf daemon instances` | Instances enregistrées ; `missing` pour une instance introuvable |
| `smf daemon run [--supervise]` | Sert au premier plan ; un premier Ctrl+C ou SIGTERM laisse finir les commandes en cours, un second arrête tout de suite |
| `smf daemon uninstall` | Arrête et retire le service ; instances, sauvegardes et registre sont conservés |

`smf doctor` signale le daemon : `ok` quand il tourne avec votre version, `warning` quand il est
absent ou arrêté (les commandes s'exécutent directement), `error` sur `version_mismatch` ou
`foreign_endpoint`.

## Mettre à jour smf

Le service lance le `smf` de l'outil : une mise à jour demande donc de redémarrer le daemon.
Jusque-là, il garde l'ancienne version et la nouvelle CLI refuse de lui envoyer des commandes
(`daemon_version_mismatch`).

Sous Windows, le daemon en marche garde les fichiers de l'outil ouverts et `dotnet tool update`
échoue (« Access to the path … is denied ») en laissant l'ancienne version en place. Un serveur
MCP lancé par Claude Code ou Codex (`smf mcp serve`) exécute le même outil : fermer aussi ces
sessions. Puis :

```text
smf daemon stop
dotnet tool update --global Sermofur
smf daemon start
```

Sous Linux et macOS, des fichiers ouverts n'empêchent pas leur remplacement : mettre à jour, puis
lancer `smf daemon restart` (ce cas n'est pas encore couvert par un test).

## Fichiers

| | Windows | Linux | macOS |
|---|---|---|---|
| Registre `instances.json` | `%APPDATA%\Sermofur` | `$XDG_CONFIG_HOME/sermofur` | `~/Library/Application Support/Sermofur` |
| Journal `daemon.log`, `service.json` | `%LOCALAPPDATA%\Sermofur` | `$XDG_STATE_HOME/sermofur` | `~/Library/Logs/Sermofur` |
| Point de connexion | pipe nommé `sermofur-<empreinte de votre SID>` | `$XDG_RUNTIME_DIR/sermofur/daemon.sock` | `$TMPDIR/sermofur-<uid>/daemon.sock` |

Le journal contient un objet JSON par ligne (heure locale avec son décalage), en trois fichiers de
1 Mio au plus. Il enregistre les événements, les codes d'erreur, les identifiants de processus des
clients et les durées, jamais les arguments ni les sorties d'une commande. Le daemon, le
superviseur Windows et les serveurs MCP l'écrivent chacun à leur tour, par `daemon.log.lock` posé à
côté ; `instances.json.lock` fait de même pour les modifications du registre. Ces deux fichiers
restent en place et ne contiennent rien. Sous Linux et macOS, eux et `daemon.lock` ne sont lisibles
que par vous (celui qu'une version plus ancienne a laissé est restreint), et un dossier de
configuration ou d'état manquant est créé privé ; un dossier existant garde ses droits.

Sous Windows, un terminal lancé par une application empaquetée (MSIX), comme certaines
applications de bureau, peut écrire `%APPDATA%` dans une copie privée de cette application : le
registre qu'il écrit est alors invisible du service, et `smf status` reste en mode direct.
Enregistrer depuis un terminal ordinaire ; `smf daemon status` donne le fichier de registre que
lit le daemon.

## Variables d'environnement

| Variable | Effet |
|---|---|
| `SERMOFUR_NO_DAEMON=1` | La CLI n'essaie jamais le daemon |
| `SERMOFUR_DAEMON_HOME=<dossier>` | Registre, journal et point de connexion sous ce dossier (tests, essais) ; la tâche planifiée de Windows ne peut pas la transmettre, `install` la refuse donc sous Windows |

Les variables relevées à l'installation (`DOTNET_ROOT`, `PATH`) parviennent au daemon : par l'unité
ou l'agent sous Linux et macOS, par le superviseur sous Windows. Un `PATH` différent ne suffit pas
à faire remplacer le service par `install`.

## Erreurs

| Code | Exit | Cas |
|---|---|---|
| `daemon_version_mismatch` | 3 | Le daemon en marche a une autre version : `smf daemon restart` s'il est le plus ancien ; sinon relancer le client (le serveur MCP dans son hôte) ou mettre smf à jour |
| `daemon_unavailable` | 3 | Service non installé (`start`, `stop`, `restart`), ou enregistré mais qui ne répond pas |
| `daemon_interrupted` | 3 | Le daemon s'est arrêté pendant une écriture, ou n'y a pas répondu en 70 s (il peut encore l'exécuter) : son résultat est inconnu, le vérifier avant de la relancer |
| `daemon_stopping` | 3 | Le daemon s'arrêtait ; la commande n'a pas commencé : la relancer |
| `daemon_busy` | 3 | Le daemon n'a pas pu commencer la commande à temps (écritures devant elle sur la même instance, ou toutes les places prises : 5 s pour une lecture, l'échéance de 60 s pour une écriture) ; la commande n'a pas commencé : la relancer (la CLI relance directement une lecture) |
| `cli_only` | 3 | Un client a envoyé au daemon une commande réservée à la CLI |
| `daemon_already_running` | 3 | `smf daemon run` alors qu'un daemon vous sert déjà |
| `service_manager_unavailable` | 3 | Aucun gestionnaire de services dans la session |
| `service_install_failed` | 3 | Le gestionnaire de services a refusé le service, ou une valeur ne peut pas entrer dans sa définition ; état précédent restauré : l'enregistrement précédent est remis en place (sous Windows, la tâche précédente est reconstruite depuis sa définition) et le daemon précédent est relancé s'il tournait (launchd le démarre en le chargeant) |
| `foreign_endpoint` | 4 | Le point de connexion ou son dossier appartient à un autre compte ou n'est pas privé |
| `invalid_registry` | 3 | Registre illisible, trop gros ou malformé ; laissé intact |
| `registry_busy` | 3 | Un autre processus `smf` modifie le registre depuis 10 s, ou (Windows) un autre programme le tient ouvert ; rien n'a changé : relancer |
| `not_registered` | 1 | `unregister` d'une instance absente du registre |
| `request_too_large` | 1 | Requête de plus de 256 Kio ; la CLI l'exécute directement |
| `response_too_large` | 1 | Sortie d'une écriture de plus de 16 Mio : elle s'est exécutée, vérifier son effet avant de la relancer (une lecture est relancée directement) |
| `request_timeout` | 3 | Commande commencée mais pas terminée 60 s après son arrivée au daemon ; elle continue et peut encore s'appliquer |
| `protocol_error` | 3 | Requête malformée |
