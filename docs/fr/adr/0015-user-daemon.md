[English](../../adr/0015-user-daemon.md) | Français

# ADR 0015-user-daemon — Un daemon par utilisateur, comme service de la session

Date : 2026-10-08. Statut : accepté, implémenté en 0.4. Remplace le « daemon machine » de
[l'ADR 0003](0003-ipc-daemon.md) ; son transport local est conservé.

## Contexte
L'ADR 0003 prévoyait un daemon machine multiplexant les instances enregistrées. Depuis
[l'ADR 0014](0014-instance-owner.md), une instance d'un autre compte n'est jamais lue : un daemon
lancé sous un compte système serait refusé par le contrôle même qui protège l'utilisateur, et un
transport réservé à l'utilisateur courant n'a de sens que si le serveur est cet utilisateur.

## Décision
- Un daemon par utilisateur et par machine, lancé sous le compte de l'utilisateur lui-même. Il ne
  sert que les instances que l'utilisateur a enregistrées par `smf daemon register` (registre dans
  le dossier de configuration de l'utilisateur, 64 Kio au plus, remplacé de façon atomique, jamais
  réécrit quand il ne peut pas être lu).
- Il est installé comme service de la session utilisateur, sans droits d'administrateur :
  - Windows : une tâche planifiée lancée à l'ouverture de session avec un jeton interactif. S4U
    n'est pas utilisé : hors domaine, il ne lance pas la tâche. Le planificateur de tâches relance
    une tâche au mieux après une minute ; la tâche exécute donc `smf daemon run --supervise`, qui
    relance le daemon en moins d'une seconde et le garde dans un job object pour qu'il ne survive
    jamais au superviseur.
  - Linux : une unité `systemd --user`, `Restart=on-failure`, `RestartSec=1`.
  - macOS : un agent launchd, `RunAtLoad`, `KeepAlive` sur échec.
- Le service lance le `smf` qui l'a installé. Après `dotnet tool update`, `smf daemon restart`
  charge la nouvelle version ; une CLI d'une autre version que le daemon refuse d'exécuter les
  commandes (`daemon_version_mismatch`) au lieu d'écrire à côté de lui.
- La désinstallation ne retire que le service : instances, sauvegardes et registre sont conservés.
- Sans gestionnaire de services (conteneur, WSL sans systemd), l'installation échoue en
  `service_manager_unavailable` ; la CLI continue de fonctionner en direct et `smf daemon run` sert
  au premier plan.

## Alternatives
Un service machine (droits d'administrateur, contredit l'ADR 0014). Un daemon lancé à la demande
par la CLI (pas de relance après un plantage, pas de démarrage avec la session). Une clé de
registre `Run` ou un cron `@reboot` (pas de relance, pas de lien avec la session).

## Conséquences
L'installation réelle est testée en CI sur les trois systèmes ; sur un poste de développement, ce
test ne tourne que sur demande (`SERMOFUR_SERVICE_TESTS=1`). Si le superviseur lui-même meurt sous
Windows, le planificateur de tâches le relance au bout d'une minute.
