[English](../release.md) | Français

# Processus de release

Sermofur suit le [versionnage sémantique](https://semver.org/lang/fr/). Les versions sont
calculées à partir des [Conventional Commits](https://www.conventionalcommits.org/fr/) qui
arrivent sur `main`, par [semantic-release](https://semantic-release.gitbook.io/), dans le
workflow `.github/workflows/ci.yml` ([ADR 0011](adr/0011-release-pipeline.md)).

## Ce qui se passe à chaque changement

| Événement | Jobs |
|---|---|
| Pull request vers `main` | `build` sous Windows, Linux et macOS : restauration verrouillée, vérification du formatage, build Release, tests, paquet (Linux) ; `release-notes` : rend des notes de version d'exemple avec l'outillage semantic-release verrouillé ; `install-rust` sur les trois mêmes systèmes : construit le `smf` Rust et teste les installeurs (voir [Binaires Rust](#binaires-rust-smf)) |
| Push sur `main` | `build`, `release-notes` et `install-rust`, puis `plan` (après `build` et `release-notes` seulement : un `install-rust` en échec ne l'arrête pas) : simulation semantic-release qui dit si une release est due |
| Release due | `release` attend l'approbation de l'environnement `release`, puis crée le tag `vX.Y.Z` et la GitHub Release ; `package` construit le tag, atteste le paquet et le joint à la release ; `publish` le pousse sur nuget.org ; `package-rust` construit le `smf` Rust du tag sur trois systèmes et `package-rust-release` atteste les archives et les joint à la release |
| Lancement manuel avec un tag | `republish` attend la même approbation ; `package` réutilise le paquet joint à cette release si ce workflow l'a attesté sur `main`, sinon construit le tag ; `publish` le pousse sur nuget.org ; `package-rust` et `package-rust-release` tournent comme ci-dessus (un tag antérieur au portage Rust n'a rien à construire et est ignoré) |
| Lancement manuel avec `dry_run`, depuis une branche `feat/**` | `package-rust-dry-run` et `package-rust-dry-run-sums` construisent et archivent le `smf` Rust à la version de `tag`, sans tag, sans release et sans attestation ; `republish` démarre aussi et est refusé aussitôt par la règle de l'environnement `release` |

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

## Binaires Rust (`smf`)

Le portage Rust est distribué sous forme de binaires joints à la GitHub Release du tag,
construits par `package-rust` (sans code npm) depuis `rust/`, avec la toolchain épinglée dans le
workflow. Une release porte :

- `smf-X.Y.Z-linux-x64.tar.gz`, `smf-X.Y.Z-macos-arm64.tar.gz` et `smf-X.Y.Z-windows-x64.zip`,
  chacune avec le binaire, `LICENSE` et `NOTICE` ;
- `SHA256SUMS`, joint en dernier : sa présence signifie que tout le reste est joint ;
- `attestations-X.Y.Z.jsonl`, l'attestation de provenance des archives, signée par `ci.yml` sur
  `main`. `package-rust-release` vérifie avec `gh attestation verify --bundle` qu'elle couvre
  chaque archive avant de rien joindre.

La version du tag arrive dans le binaire à la compilation (`SMF_BUILD_VERSION`) ; `smf --version`
affiche `Sermofur X.Y.Z`, et le script de packaging refuse un binaire qui rapporte une autre
version. Le pipeline ne modifie jamais `Cargo.toml`.

Installation (Linux x64, macOS arm64 ; Windows x64 avec `install.ps1`) :

```sh
curl -fsSL https://raw.githubusercontent.com/JoRouquette/sermofur/main/rust/dist/install.sh | sh
sh install.sh v1.0.0        # un tag donné
```

Les deux installeurs contrôlent l'archive contre `SHA256SUMS`, puis, quand un `gh` récent est
installé, contre l'attestation, et n'installent rien si l'un des deux contrôles échoue ; la version que
rapporte le binaire doit égaler le tag. Sans `gh` utilisable, ils avertissent et ne contrôlent que
la somme, ce qui protège d'un téléchargement corrompu mais pas de qui peut écrire sur la Release.
Voir l'[ADR 0011](adr/0011-release-pipeline.md). Variables d'environnement : `SMF_INSTALL_DIR`,
`SMF_RELEASES_URL`, `SMF_VERSION`, `SMF_REQUIRE_ATTESTATION=1` (refuser si `gh` manque),
`SMF_SKIP_ATTESTATION=1` (installer sur la somme seule, avec un avertissement ; refusée avec la
précédente), `SMF_GH` (l'exécutable `gh`) et, pour `install.ps1`, `SMF_NO_MODIFY_PATH=1`.

Pour essayer le packaging avant une release : Actions → *CI and release* → *Run workflow*,
**depuis une branche `feat/**`**, `tag` = la version à construire (par exemple `v1.0.0`),
`dry_run` coché. Les archives et `SHA256SUMS` sont des artefacts du run. Ne jamais lancer
`dry_run` depuis `main` : `republish` y serait réel.

## Réglages initiaux (mainteneur)

Ces réglages vivent hors du dépôt et se font une fois.

1. **Protéger `main`** (Settings → Rules → Rulesets → New branch ruleset, cible `main`) : pull
   request obligatoire (0 approbation pour un mainteneur seul, qui ne peut pas approuver sa propre
   pull request) ; checks requis `build (windows-latest)`, `build (ubuntu-latest)`,
   `build (macos-latest)`, `release-notes` et les trois checks `install-rust`
   (`install-rust (ubuntu-latest, linux-x64)`, `install-rust (windows-latest, windows-x64)`,
   `install-rust (macos-latest, macos-arm64)`) ; force push et suppression interdits. Dans
   Settings → General, autoriser le merge commit et le rebase, désactiver le squash. Les checks
   Rust de `rust.yml` sont filtrés par chemin : ne pas les exiger, sinon une pull request qui ne
   touche pas `rust/` les attendrait indéfiniment.
2. **Protéger les tags de release** (New tag ruleset, cible `v*`) : mise à jour et suppression
   interdites, pour qu'un tag publié ne puisse jamais être déplacé ni retiré.
3. **Environnement `release`** (Settings → Environments → New environment) : relecteur requis =
   le mainteneur ; branches et tags de déploiement = `main` seulement. Il conditionne le tag et
   la publication.
4. **Environnement `nuget`** : sans relecteur ; branches et tags de déploiement = `main`
   seulement. C'est le seul environnement auquel nuget.org fait confiance.
5. **Trusted Publishing nuget.org** (nuget.org → votre nom d'utilisateur → Trusted Publishing →
   ajouter une politique) : propriétaire `JoRouquette`, dépôt `sermofur`, fichier de workflow
   `ci.yml`, environnement `nuget` ; portée *Push new packages and package versions* (le premier
   push crée le paquet), *Unlist or relist* décoché ; motifs et paquets : `Sermofur` seulement.
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
- **`package-rust` ou `package-rust-release` a échoué** : lancer `republish` avec ce tag. Les
  archives jointes par un run interrompu sont remplacées. Si la release porte déjà `SHA256SUMS`,
  le job garde les assets seulement quand les trois archives, leurs sommes et leurs attestations
  (par l'API et par le fichier joint) concordent ; sinon il s'arrête. Pour reconstruire une telle
  release, supprimer `SHA256SUMS` de la release, puis relancer `republish`.
- **Ne jamais** supprimer un tag publié ni réutiliser un numéro de version : publier un nouveau
  correctif.
- Un run de release arrêté avant le tag : rien n'a été publié ; le prochain push sur `main`
  recalcule la release.
