[English](../../adr/0011-release-pipeline.md) | Français

# ADR 0011-release-pipeline — Versionnage sémantique et chaîne de release

Date : 2026-10-07. Statut : accepté.

## Contexte
Sermofur est publié comme outil .NET. Les versions étaient fixées à la main dans le fichier de
projet, rien ne vérifiait le code hors Windows, et publier passait par des commandes locales avec
une clé nuget.org de longue durée. Les commits suivent déjà les Conventional Commits.

## Décision
- Les versions suivent le versionnage sémantique et sont calculées par semantic-release à partir
  des commits de `main` (`fix`/`perf`/`revert` correctif, `feat` mineure, rupture majeure ; les
  autres types ne publient rien). Les tags sont `vX.Y.Z`.
- Un seul workflow, `ci.yml`, construit et teste chaque pull request et chaque push sous Windows,
  Linux et macOS. Sur `main`, une simulation décide si une release est due ; le job `release`
  attend ensuite l'approbation du mainteneur dans l'environnement `release`, tague et crée la
  GitHub Release.
- Le job `package` construit le tag (qui doit être sur `main`) sans aucun code npm, atteste la
  provenance du paquet et le joint à la release. Le job `publish` le pousse sur nuget.org.
- La publication passe par le Trusted Publishing de nuget.org (OIDC) : aucune clé de longue durée
  n'est stockée. Seul le job `publish`, dans l'environnement `nuget` que nomme la politique
  nuget.org, peut obtenir une clé nuget.org ; il n'exécute ni code du dépôt, ni npm, ni build.
  Les jobs semantic-release ne détiennent qu'un jeton GitHub.
- La chaîne n'écrit jamais de commit sur `main` : la version est passée au build et les notes de
  version vivent dans les GitHub Releases. Le dépôt garde la version de développement
  `0.0.0-dev`, et l'outil lit sa version dans son assembly.
- Un lancement manuel republie un tag existant (première release, publication échouée) ; il passe
  par la même approbation. Il réutilise le paquet joint à la release seulement si ce workflow
  l'a attesté, et sinon construit le tag : l'asset de la release et nuget.org portent toujours
  les mêmes octets.

## Alternatives
release-please : une pull request de release donne un point de revue, mais ajoute une pull
request de robot à chaque changement ; l'approbation de l'environnement `release` donne le même
contrôle. Committer la version et un changelog sur `main` (plugin git de semantic-release) :
demande un jeton qui contourne la protection de branche. Un tag manuel avec MinVer : pas de
version calculée, pas de notes de version. Construire le paquet dans semantic-release (plugin
exec) : l'outillage npm produirait les octets publiés.

## Conséquences
Les contributeurs doivent suivre les Conventional Commits, et les pull requests ne sont pas
fusionnées en squash. Node n'est nécessaire qu'en CI. Une rupture en 0.x publie la 1.0.0. Chaque
release demande une approbation ; une release refusée est incluse dans la suivante. Le nom du
fichier de workflow et l'environnement `nuget` font partie de la politique nuget.org.

Risque accepté : la simulation `plan` a besoin de `contents: write` (semantic-release vérifie
qu'il pourrait pousser le tag) et exécute l'outillage npm à chaque push sur `main`, sans
approbation. Une dépendance compromise pourrait alors créer un tag sur un commit de `main`, une
GitHub Release ou un asset, mais pas modifier le paquet publié : `package` ne réutilise un asset
que si ce workflow l'a attesté sur `main` pour cette version, et sinon s'arrête, ou construit le
tag, qui doit être sur `main`, avec le script de packaging de `main` ; `publish` ne pousse que ce
paquet. Les dépendances sont épinglées, verrouillées et installées sans
scripts.
