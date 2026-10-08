[English](../../adr/0016-ipc-protocol.md) | Français

# ADR 0016-ipc-protocol — Protocole local 1 et routage de la CLI

Date : 2026-10-08. Statut : accepté, implémenté en 0.4. Complète
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
- Trames : une longueur `uint32` petit-boutiste, puis un corps JSON UTF-8 de 256 Kio au plus. Une
  session s'ouvre par `hello` (protocole, version de l'outil, dossier de lancement) ; le scope de
  la session vient de ce dossier et aucune requête ne peut le changer. `run` porte les arguments ;
  le daemon les exécute avec le même `CommandRunner` que la CLI, à partir du dossier de lancement,
  et rend le code de sortie, stdout et stderr tels que la CLI les aurait écrits.
- Les écritures sur une instance passent une à une dans le daemon ; les lectures s'exécutent en
  parallèle. Un client qui s'en va annule sa commande en file d'attente ; une commande commencée
  termine sa transaction. Les rejeux reposent sur les clés d'idempotence du moteur (`--key`), pas
  sur un cache du daemon.
- `shutdown` est accepté avant tout `hello` et depuis toute version, pour qu'un `smf` plus récent
  puisse arrêter un daemon plus ancien.
- Routage dans la CLI : `daemon …`, `init`, `doctor`, l'aide et la version s'exécutent toujours
  dans le processus. Sinon, pas de point de connexion → direct, au prix d'une recherche dans le
  système de fichiers ; un daemon d'une autre version → `daemon_version_mismatch` (exit 3) ; une
  instance non enregistrée → direct. `SERMOFUR_NO_DAEMON=1` force l'exécution directe ;
  `SERMOFUR_DAEMON_HOME` place configuration, état et point de connexion sous un seul dossier
  (tests, essais).
- Sous Windows, le point de connexion est recherché en listant le dossier des pipes : ouvrir le
  pipe pour lire ses attributs consommerait une instance du serveur.

## Alternatives
JSON-RPC (aucun gain tant que la CLI est le seul client). Passer le dossier par `--path` (conflit
avec un `--path` déjà donné). Exécuter les commandes directement à côté d'un daemon d'une autre
version (deux écrivains de versions différentes).

## Conséquences
Un test rejoue de vraies commandes CLI avec et sans le daemon et compare les sorties octet par
octet ; un test de charge mesure le surcoût (2,9 ms au 95e percentile sous Windows).
