[English](../../adr/0016-ipc-protocol.md) | Français

# ADR 0016-ipc-protocol — Protocole local 1 et routage de la CLI

Date : 2026-10-08, écritures de configuration ajoutées le 2026-10-12. Statut : accepté,
implémenté en 0.4 et 0.4.3. Complète
[l'ADR 0003](0003-ipc-daemon.md) et [l'ADR 0015](0015-user-daemon.md).

## Contexte
La CLI doit donner le même résultat avec et sans le daemon, et le daemon doit appliquer exactement
les règles du moteur. Le pont MCP sera un second client du même daemon.

## Décision
- Point de connexion : un pipe nommé réservé à l'utilisateur courant sous Windows
  (`PipeOptions.CurrentUserOnly` des deux côtés) ; ailleurs, un socket de domaine Unix `0600` dans
  un dossier `0700`, le serveur contrôlant l'identifiant utilisateur de chaque pair
  (`SO_PEERCRED`, `getpeereid`), le client le propriétaire et le mode du dossier et du socket.
  Aucun port réseau. Un point de connexion d'un autre compte donne `foreign_endpoint`.
- Trames : une longueur `uint32` petit-boutiste, puis un corps JSON UTF-8 sans échappement ASCII,
  de 256 Kio au plus pour ce que lit le daemon et de 16 Mio pour ce que lit un client (une liste,
  un export). Une session s'ouvre par `hello` (protocole, version de l'outil, dossier de
  lancement) ; le dossier de lancement de la session ne change jamais, et `--path` est respecté
  comme dans la CLI directe (le pont MCP ne l'envoie jamais). `run` porte un identifiant et les
  arguments ; le daemon les exécute avec le même `CommandRunner` que la CLI, à partir du dossier de
  lancement, et rend, sous le même identifiant, le code de sortie, stdout et stderr tels que la
  CLI les aurait écrits. Une sortie au-delà de la limite donne une erreur `response_too_large`
  avec l'identifiant ; la session continue.
- Le daemon contrôle d'abord la version de l'outil, puis le protocole : un client d'une autre
  version reçoit toujours `daemon_version_mismatch`, quel que soit le protocole de chacun.
- Les écritures sur une instance passent une à une dans le daemon ; les lectures s'exécutent en
  parallèle. Au plus une commande par cœur du processeur moins un (deux au moins) s'exécute à la
  fois, chacune sur un fil qui lui est propre : hellos, statut et arrêt n'attendent jamais derrière
  elles. Une échéance par requête, à partir de son arrivée : une commande dont le tour ou la place
  n'est pas venu dans le délai de requête, ou vient alors qu'il en reste moins d'un vingtième,
  reçoit `daemon_busy` et n'a pas commencé ; une lecture attend une place 5 s au plus, puis reçoit
  aussi `daemon_busy` (la CLI relance directement une lecture ; le pont MCP, qui n'exécute jamais
  en direct, transmet l'erreur à son hôte, qui rappelle) ; une commande
  commencée et pas terminée à ce moment reçoit `request_timeout` et continue. La CLI laisse au
  daemon le délai de requête plus 10 s, puis tient la session pour perdue : une lecture est
  relancée directement, une écriture reçoit `daemon_interrupted`. Le journal s'écrit en
  arrière-plan et ne retient jamais une requête.
  Un client qui s'en va annule sa commande en file d'attente ; une commande commencée
  termine sa transaction. Un arrêt cesse aussitôt de servir de nouveaux clients (sous Unix le
  socket est supprimé ; sous Windows, où un nom de pipe vit autant que ses instances, une instance
  continue d'accepter et ferme chaque connexion sans un mot, si bien qu'un client conclut
  immédiatement « aucun daemon »), puis laisse les commandes commencées répondre avant que le
  daemon ne se termine (un délai de requête plus 5 s) ; une commande qui attend encore son tour ou
  sa place reçoit `daemon_stopping`. Une commande encore en cours à la fin de ce délai est coupée, et sa
  transaction annulée. Un second Ctrl+C ou SIGTERM termine le processus aussitôt. Le daemon garde
  son fichier de verrou jusqu'à sa fin : `smf daemon stop`, `restart`, `install` et `uninstall`
  attendent ce verrou (même quand le point de connexion est déjà fermé, en renvoyant l'arrêt à un
  daemon qui se met à écouter en retard) avant d'appeler le gestionnaire de services ; systemd et
  launchd laissent le même délai ; le superviseur Windows l'attend sur un premier Ctrl+C ou
  SIGTERM, mais une tâche terminée hors de `smf` (Planificateur de tâches, fin de session) arrête
  le daemon tout de suite. Les rejeux reposent sur les clés d'idempotence du moteur (`--key`), pas
  sur un cache du daemon.
- `shutdown` est accepté avant tout `hello` et depuis toute version, pour qu'un `smf` plus récent
  puisse arrêter un daemon plus ancien.
- Routage dans la CLI : `daemon …`, `mcp …`, `init`, `doctor`, l'aide et la version s'exécutent
  toujours dans le processus, et le daemon les refuse quel que soit le client (`cli_only`). Sinon,
  pas de point de connexion → direct, au prix d'une recherche dans le système de fichiers ; tout
  refus du `hello` par le daemon (autre version, autre protocole) → l'erreur, exit 3, jamais
  d'exécution directe ; une instance non enregistrée → direct. Une fois un `run` envoyé, une
  session perdue sans réponse donne `daemon_interrupted` : une lecture est relancée directement,
  une écriture n'est jamais exécutée deux fois. `SERMOFUR_NO_DAEMON=1` force l'exécution directe ;
  `SERMOFUR_DAEMON_HOME` place configuration, état et point de connexion sous un seul dossier
  (tests, essais).
- Sous Windows, le point de connexion est recherché en listant le dossier des pipes : ouvrir le
  pipe pour lire ses attributs consommerait une instance du serveur.

## Écritures de configuration (0.4.3)
Tout fichier de configuration est remplacé d'un seul coup : un fichier temporaire à nom aléatoire
dans le même dossier, renommé par-dessus la cible. Un renommage par-dessus un lien remplace le lien
lui-même, donc le vrai fichier n'est jamais écrit ; le mode Unix lu à travers un lien est celui de
sa cible, donné ensuite à un fichier ordinaire. Les fichiers écrits ainsi n'ont pas le même
propriétaire, donc une écriture déclare ce qu'un lien à la place de sa cible signifie.
- **Refusé** pour l'état de sermofur (`instances.json`, `service.json`) : `unsafe_path` (exit 4),
  rien d'écrit. Aucun lien à cet endroit n'a été posé par le produit, et le suivre écrirait hors du
  dossier dont il est propriétaire. Ce que cela voit, c'est un lien symbolique ou une jonction ; un
  lien physique est un autre nom du même fichier, qu'aucun attribut ne distingue, donc une écriture
  à travers lui ne remplace que ce nom-là. Les deux écrivains portent cette politique sans qu'on
  leur dise, et un test passe par chacun d'eux.
- **Suivi** pour la configuration d'un hôte (`.mcp.json`, le `config.toml` de Codex) : la chaîne de
  liens est résolue et c'est sa cible finale qui est remplacée, donc le lien reste un lien et le
  fichier reste dans le dépôt où un gestionnaire de dotfiles l'a mis. Refuser casserait une
  installation qui marche ; écrire par-dessus le lien sortirait le fichier de ce dépôt sans un mot.
- Une chaîne suivie est refusée, chacune par sa propre raison, quand elle ne mène nulle part, vers
  un dossier, en boucle, ou par plus de dix maillons : écrire quand même laisserait un fichier que
  l'hôte ne lit pas. Une chaîne qui revient sur un chemin déjà parcouru est nommée boucle ; une
  chaîne qui continue est nommée trop longue.
- Une chaîne suivie est suivie où qu'elle mène : rien n'exige que sa cible finale reste dans le
  projet ou dans un dossier de configuration. Un lien porté par un dépôt peut donc envoyer
  l'écriture ailleurs, aussi l'écriture ne tait jamais où elle a atterri : `smf mcp install` et
  `uninstall` nomment le fichier réellement remplacé dès qu'il n'est pas celui demandé.
- Le mode Unix est lu sur la cible finale et lui est rendu, jamais à travers le lien.
- Seule la cible est examinée, pas les dossiers au-dessus : un dossier du chemin est un lien sur un
  système sain (`/var` sous macOS), et les refuser refuserait au daemon son propre état. La
  découverte d'une instance continue de refuser tout lien sur son chemin (ADR 0010) : cette règle
  porte sur ce qui peut être lu, pas sur la façon de remplacer un fichier.
- Un seul écrivain porte cela (`AtomicFile`, dans Infrastructure), et les écritures d'instance de
  l'Infrastructure (configuration d'une instance, projections Markdown) l'utilisent au lieu de leur
  propre fichier temporaire.
- Le superviseur ne transmet au daemon que les variables portées (`DOTNET_ROOT`, `PATH`,
  `SERMOFUR_DAEMON_HOME`), chemins absolus seulement et `PATH` réduit à ses entrées absolues :
  `service.json` est un fichier de l'utilisateur, et un chemin relatif serait lu depuis le dossier
  de travail du daemon. Une ligne de journal nomme chaque variable écartée, jamais sa valeur. Un
  `service.json` dont la forme ne tient pas se lit comme « rien d'installé », au lieu d'échouer
  hors des erreurs métier.

## Alternatives
JSON-RPC (aucun gain tant que la CLI est le seul client). Passer le dossier par `--path` (conflit
avec un `--path` déjà donné). Exécuter les commandes directement à côté d'un daemon d'une autre
version (deux écrivains de versions différentes).

## Conséquences
Les cas d'une chaîne de liens refusée ou suivie, les droits d'une cible finale, l'environnement
filtré et un `service.json` dont la forme ne tient pas sont couverts par des tests sur les trois
systèmes ; sous Windows, un lien symbolique vers un fichier exige une session élevée ou le mode
développeur, donc ces faits sont rapportés comme ignorés sans l'un des deux.
Un test rejoue de vraies commandes CLI avec et sans le daemon et compare les sorties octet par
octet ; un test de charge mesure le surcoût (2,9 ms au 95e percentile sous Windows).
