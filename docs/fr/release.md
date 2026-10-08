[English](../release.md) | Français

# Processus de release

Sermofur suit le [versionnage sémantique](https://semver.org/lang/fr/). Les versions sont
calculées à partir des [Conventional Commits](https://www.conventionalcommits.org/fr/) qui
arrivent sur `main`, par [semantic-release](https://semantic-release.gitbook.io/), dans le
workflow `.github/workflows/ci.yml` ([ADR 0011](adr/0011-release-pipeline.md)).

## Ce qui se passe à chaque changement

| Événement | Jobs |
|---|---|
| Pull request vers `main` | `build` sous Windows, Linux et macOS : restauration verrouillée, vérification du formatage, build Release, tests, paquet (Linux) |
| Push sur `main` | `build`, puis `plan` : simulation semantic-release qui dit si une release est due |
| Release due | `release` attend l'approbation de l'environnement `release`, puis crée le tag `vX.Y.Z` et la GitHub Release ; `package` construit le tag, atteste le paquet et le joint à la release ; `publish` le pousse sur nuget.org |
| Lancement manuel avec un tag | `republish` attend la même approbation ; `package` réutilise le paquet joint à cette release si ce workflow l'a attesté sur `main`, sinon construit le tag ; `publish` le pousse sur nuget.org |

| Commit sur `main` | Version |
|---|---|
| `fix:`, `perf:`, `revert:` | correctif |
| `feat:` | mineure |
| `!` après le type, ou un pied `BREAKING CHANGE:` | majeure (même depuis 0.x : semantic-release passe alors à 1.0.0) |
| `docs:`, `test:`, `refactor:`, `build:`, `ci:`, `style:`, `chore:` | pas de release |

La chaîne n'écrit jamais de commit sur `main` : la version est passée au build
(`-p:Version=X.Y.Z`) et les notes de version vivent dans les GitHub Releases. Dans le dépôt, la
version reste `0.0.0-dev` (`Directory.Build.props`) ; `smf --help` affiche la version du build
et renvoie à la référence CLI de son tag, ou de `main` pour un build de développement.

Le paquet est construit depuis le tag par `package`, qui n'exécute aucun code npm, et seul
`publish` peut obtenir une clé nuget.org : il n'exécute ni code du dépôt, ni npm, ni build. Les
jobs qui lancent semantic-release et ses dépendances npm ne détiennent qu'un jeton GitHub.

Dependabot propose chaque semaine les mises à jour des actions et des paquets NuGet et npm. Ses
mises à jour NuGet portent le type `build:`, qui ne déclenche pas de release. Quand une mise à
jour corrige une vulnérabilité d'une dépendance d'exécution, ajouter à la branche de Dependabot,
avant la fusion, un commit `fix(deps): ...` qui nomme l'avis, pour qu'une version corrective livre
le correctif.

## Réglages initiaux (mainteneur)

Ces réglages vivent hors du dépôt et se font une fois.

1. **Protéger `main`** (Settings → Rules → Rulesets → New branch ruleset, cible `main`) : pull
   request obligatoire ; checks requis `build (windows-latest)`, `build (ubuntu-latest)` et
   `build (macos-latest)` ; force push et suppression interdits. Dans Settings → General,
   autoriser le merge commit et le rebase, désactiver le squash.
2. **Protéger les tags de release** (New tag ruleset, cible `v*`) : mise à jour et suppression
   interdites, pour qu'un tag publié ne puisse jamais être déplacé ni retiré.
3. **Environnement `release`** (Settings → Environments → New environment) : relecteur requis =
   le mainteneur ; branches et tags de déploiement = `main` seulement. Il conditionne le tag et
   la publication.
4. **Environnement `nuget`** : sans relecteur ; branches et tags de déploiement = `main`
   seulement. C'est le seul environnement auquel nuget.org fait confiance.
5. **Trusted Publishing nuget.org** (nuget.org → votre nom d'utilisateur → Trusted Publishing →
   ajouter une politique) : propriétaire `JoRouquette`, dépôt `sermofur`, fichier de workflow
   `ci.yml`, environnement `nuget`.
6. **Secret `NUGET_USER`** (Settings → Secrets and variables → Actions, ou secret de
   l'environnement `nuget`) : le nom de profil nuget.org (pas l'adresse mail). Ce n'est pas un
   identifiant secret : nuget.org délivre au workflow une clé valable une heure en échange de son
   jeton OIDC.

## Première release (0.1.0)

semantic-release commence à 1.0.0 quand aucun tag n'existe. La première version est donc taguée
à la main sur le commit déjà publié sur `main`, **avant** de fusionner la pull request qui ajoute
la chaîne :

```powershell
git tag -a v0.1.0 -m "Sermofur 0.1.0" <commit>
git push origin v0.1.0
```

Puis, dans cet ordre :

1. Fusionner la pull request qui ajoute la chaîne. Le push sur `main` lance `plan`, qui peut
   proposer la version suivante (0.1.1 pour un `fix:`) ; **refuser** ce job `release`.
2. Actions → *CI and release* → *Run workflow*, branche `main`, tag `v0.1.0` ; approuver le job
   `republish` et attendre que `publish` ait poussé 0.1.0.
3. Ouvrir le run refusé à l'étape 1 et lancer *Re-run all jobs* : `plan` recalcule la version
   suivante à partir de `v0.1.0`, et `release` attend l'approbation. Si `main` a avancé entre-temps,
   passer cette étape : le prochain push porte la release.

Les releases suivantes partent de ce tag. Si un job `release` propose un jour 1.0.0 par erreur,
le refuser. Les notes de cette première release sont générées par GitHub, pas par
semantic-release.

## Approuver une release

Quand une release est due, le run affiche le job `release` en attente de revue. Vérifier la
version calculée par `plan` (dans son journal) et les commits depuis le dernier tag, puis
approuver. Refuser pour sauter la release : les commits restent sur `main` et la release
suivante les inclut.

Si `main` a avancé pendant l'attente de l'approbation, semantic-release ne publie rien et le job
échoue volontairement (« nothing was released ») : c'est le run du push plus récent qui porte la
release.

## Reprise

- **`publish` a échoué** (nuget.org indisponible, politique absente) : lancer *Re-run failed
  jobs* sur ce run tant que son artefact `release-package` existe ; le même paquet est poussé.
- **L'artefact a expiré, ou `package` a échoué** : lancer `republish` avec ce tag. Un paquet déjà
  joint à la release est réutilisé après vérification de son attestation : l'asset de la release
  et nuget.org gardent les mêmes octets ; un paquet que ce workflow n'a pas attesté arrête le
  job. Sans paquet, le tag est construit, attesté et joint. Le push utilise `--skip-duplicate` :
  une version déjà sur nuget.org n'est pas une erreur. Ne pas relancer le job `release` : le tag
  existe, il ne publierait rien.
- **Ne jamais** supprimer un tag publié ni réutiliser un numéro de version : publier un nouveau
  correctif.
- Un run de release arrêté avant le tag : rien n'a été publié ; le prochain push sur `main`
  recalcule la release.
