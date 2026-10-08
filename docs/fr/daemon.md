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

Celles-ci s'exécutent toujours dans la CLI elle-même : `smf daemon …`, `init`, `doctor`,
`--help`, `--version`. Les commandes pour une instance non enregistrée s'exécutent directement.

| Commande | Effet |
|---|---|
| `smf daemon status [--json]` | `absent`, `installed_stopped`, `running`, `version_mismatch`, `foreign_endpoint`, `service_manager_unavailable` ; version, processus, démarrage, instances ouvertes, clients |
| `smf daemon start` / `stop` / `restart` | Pilote le service installé |
| `smf daemon register` / `unregister` | Ajoute ou retire l'instance trouvée depuis le dossier (ou `--path`) |
| `smf daemon instances` | Instances enregistrées ; `missing` pour une instance introuvable |
| `smf daemon run [--supervise]` | Sert au premier plan, jusqu'à Ctrl+C |
| `smf daemon uninstall` | Arrête et retire le service ; instances, sauvegardes et registre sont conservés |

`smf doctor` signale le daemon : `ok` quand il tourne avec votre version, `warning` quand il est
absent ou arrêté (les commandes s'exécutent directement), `error` sur `version_mismatch` ou
`foreign_endpoint`.

## Mettre à jour smf

Le service lance le `smf` de l'outil : une mise à jour demande donc de redémarrer le daemon.
Jusque-là, il garde l'ancienne version et la nouvelle CLI refuse de lui envoyer des commandes
(`daemon_version_mismatch`).

Sous Windows, le daemon en marche garde les fichiers de l'outil ouverts et `dotnet tool update`
échoue (« Access to the path … is denied ») en laissant l'ancienne version en place. L'arrêter
d'abord :

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
clients et les durées, jamais les arguments ni les sorties d'une commande.

## Variables d'environnement

| Variable | Effet |
|---|---|
| `SERMOFUR_NO_DAEMON=1` | La CLI n'essaie jamais le daemon |
| `SERMOFUR_DAEMON_HOME=<dossier>` | Registre, journal et point de connexion sous ce dossier (tests, essais) |

## Erreurs

| Code | Exit | Cas |
|---|---|---|
| `daemon_version_mismatch` | 3 | Le daemon en marche a une autre version : `smf daemon restart` |
| `daemon_unavailable` | 3 | Service non installé (`start`, `stop`, `restart`), ou enregistré mais qui ne répond pas |
| `daemon_already_running` | 3 | `smf daemon run` alors qu'un daemon vous sert déjà |
| `service_manager_unavailable` | 3 | Aucun gestionnaire de services dans la session |
| `service_install_failed` | 3 | Le gestionnaire de services a refusé le service ; état précédent restauré |
| `foreign_endpoint` | 4 | Le point de connexion ou son dossier appartient à un autre compte ou n'est pas privé |
| `invalid_registry` | 3 | Registre illisible, trop gros ou malformé ; laissé intact |
| `not_registered` | 1 | `unregister` d'une instance absente du registre |
| `request_too_large` | 1 | Requête de plus de 256 Kio |
| `request_timeout` | 3 | Commande de plus de 60 s ; sa transaction se termine quand même |
| `protocol_error` | 3 | Requête malformée |
