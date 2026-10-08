[English](README.md) | Français

# Sermofur

[![CI and release](https://github.com/JoRouquette/sermofur/actions/workflows/ci.yml/badge.svg)](https://github.com/JoRouquette/sermofur/actions/workflows/ci.yml)
[![NuGet](https://img.shields.io/nuget/v/Sermofur)](https://www.nuget.org/packages/Sermofur)

Sermofur est un runtime cognitif local : une mémoire qu'un assistant, ou vous-même, pouvez consulter
et **contester**. Il ne stocke pas des « faits » ; il stocke des **claims** (affirmations)
appuyées par des **preuves** (evidence) et des **RETEX** (retours d'expérience), chacun dans un
**scope** isolé, avec leur **provenance** et une **confiance expliquée**. Le but est d'améliorer
les décisions futures grâce à l'expérience, sans faire des erreurs passées des vérités
permanentes.

Sermofur fonctionne à côté du modèle de langage, jamais dedans : les poids du LLM hôte ne sont
jamais modifiés. Sermofur est indépendant de tout fournisseur de LLM.

Le nom vient du monde de jeu de rôle de l'auteur, où le Sermofur est une écriture gravée qui garde
ce que l'on oublie : *« Quand l'œil oublie, la main se souvient. »*

Identité technique : dépôt `sermofur`, projets .NET `Sermofur.*`, paquet d'outil NuGet
`Sermofur`, commande `smf`, marqueur d'instance `.sermofur`.

## Modèle cognitif

USER ≠ TRUTH, LLM ≠ TRUTH, MEMORY ≠ TRUTH.

- Un **claim** est une affirmation dans un scope. Sa confiance est calculée à la lecture, à partir
  de ses preuves, et chaque niveau est accompagné de ses raisons.
- Une **preuve** est un support déclaré d'un claim (code source, documentation, observation…).
  Les preuves de même lignée comptent pour une seule origine : la répétition n'augmente pas la
  confiance. Une preuve déclarée par un LLM ne renforce jamais un claim, et une preuve
  `execution` déclarée ne suffit pas à vérifier un fait.
- Une preuve peut **contredire** un claim : une contradiction indépendante plafonne sa confiance à
  medium ; celle d'un LLM est notée mais ne réfute rien.
- Une **source** est un fichier texte local que vous déclarez explicitement : Sermofur garde son
  empreinte, indexe son texte et sait quand une preuve repose sur un fichier modifié depuis.
- Le **recall** répond à une question par au plus 3 résultats expliqués ; le **challenge**
  confronte un claim à ce qui le contredit, sans jamais trancher à votre place.
- Un **RETEX** reste un brouillon tant qu'aucune décision d'apprentissage n'est prise.
- Les **scopes** (workspace → client → project → repository → task) isolent les mémoires : un
  contexte voit son scope et ses ancêtres, jamais ses frères ni ses descendants.
- Chaque modification est conservée dans un **historique** auditable.

## État

Livré : la CLI `smf`, les instances locales, les scopes, le stockage Claim/Evidence/RETEX en
SQLite, l'historique, les projections Markdown et les diagnostics (`doctor`) ; depuis la 0.2, les
sources déclarées avec index plein texte, le recall, le challenge et les preuves contraires
(format d'instance 2) ; depuis la 0.4, un [daemon](docs/fr/daemon.md) facultatif par
utilisateur, installé comme service de votre session (`smf daemon install`), qui sert les
instances que vous enregistrez à toutes les commandes `smf`.

**Pas encore livré** : pont MCP, intégration Laya (modèle System 1),
apprentissage et consolidation, interface desktop Inspector. La documentation les décrit comme
conceptions seulement ; aucune commande ne prétend les fournir.

Aucune télémétrie, synchronisation ni connexion réseau à l'exécution, et aucune ingestion
implicite : seuls les fichiers que vous ajoutez comme sources sont lus.

Mise à jour depuis la 0.1 : le format d'instance a changé. Les autres commandes refusent une
instance 0.1 tant que vous n'avez pas lancé `smf migrate`, qui sauvegarde d'abord la base sous
`.sermofur/backups/`.

## Installer

Avec le [SDK .NET 10](https://dotnet.microsoft.com/download), depuis nuget.org (l'exécution de
l'outil ne demande ensuite que le runtime .NET 10) :

```powershell
dotnet tool install --global Sermofur
smf --help
```

Versions et notes de version : [GitHub Releases](https://github.com/JoRouquette/sermofur/releases).

## Installer depuis les sources

Prérequis : le [SDK .NET 10](https://dotnet.microsoft.com/download). Un build depuis les sources
porte la version de développement `0.0.0-dev`.

```powershell
git clone https://github.com/JoRouquette/sermofur.git
cd sermofur
dotnet restore --locked-mode
dotnet build --no-restore
dotnet pack src/Sermofur.Cli -c Release -o artifacts/packages --no-restore
dotnet tool install Sermofur --version 0.0.0-dev --tool-path artifacts/tools --configfile nuget.local.config --no-cache
./artifacts/tools/smf --help
```

`nuget.local.config` ne liste que le dossier de paquets local : l'installation ne prend jamais un
paquet du même nom sur nuget.org. `--global` à la place de `--tool-path artifacts/tools` place
`smf` dans le `PATH`. L'outil exige le runtime .NET 10 ; une distribution autonome est prévue.
Sans installation, `dotnet run --project src/Sermofur.Cli -- --help` exécute la CLI depuis les
sources.

## Démarrage rapide

```powershell
cd ~/work                        # ce dossier devient la racine de l'instance
smf init                         # crée ~/work/.sermofur
mkdir acme
smf scope add acme client workspace acme
cd acme                          # le contexte est maintenant le scope « acme »
smf claim add "The billing API paginates with cursors" --origin user --json
smf evidence add <CLAIM_ID> source_code "src/Billing/Pagination.cs" --lineage billing-repo --origin user
smf claim show <CLAIM_ID>        # claim, preuves, confiance expliquée, historique
smf source add notes/billing.md --origin user    # source déclarée, hachée et indexée
smf recall "billing pagination"  # au plus 3 résultats expliqués, ce scope et ses ancêtres
smf challenge <CLAIM_ID>         # contradictions, sources modifiées, claims proches à confronter
smf doctor                       # diagnostic en lecture seule
smf export                       # reconstruit les projections Markdown des objets visibles
```

`<CLAIM_ID>` est l'`id` rendu par `claim add`. Le scope vient toujours du dossier de travail (ou
de `--path`), jamais d'un argument : depuis le dossier d'un autre client, le claim d'`acme` est
invisible. `--origin user|llm` est obligatoire sur chaque `add` qui enregistre de la mémoire.
`--json` donne une sortie lisible par machine sur toute commande. Référence complète :
[CLI](docs/fr/cli.md).

## Documentation

- [Architecture](docs/fr/architecture.md), [environnement de développement](docs/fr/developer-setup.md),
  [vérification](docs/fr/verification.md).
- [CLI](docs/fr/cli.md), [format d'instance](docs/fr/instance-format.md),
  [modèle de scopes](docs/fr/scope-model.md).
- [Modèle mémoire](docs/fr/memory-model.md), [modèle de sécurité](docs/fr/security-model.md).
- [MCP](docs/fr/mcp-integration.md) et [Laya](docs/fr/laya-integration.md) : contrats futurs,
  pas des implémentations.
- [Processus de release](docs/fr/release.md) : versions, CI, publication.
- Décisions d'architecture : [docs/fr/adr](docs/fr/adr/).

Les spécifications sont rédigées dans un atelier Spec Kit tenu hors de ce dépôt ; les décisions
durables en sont extraites sous forme d'ADR.

## Encodage de la sortie

stdout et stderr sont en UTF-8 sans BOM. `smf` ne bascule la page de code de la console que si
stdout ou stderr atteint la console, et la restaure à la fin normale, sur erreur gérée et sur
Ctrl+C ; la restauration n'est pas garantie si le processus est tué. Sous Windows PowerShell, un
appelant qui capture la sortie exécute d'abord
`[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)`. Détail : [CLI](docs/fr/cli.md).

## Contribuer

Les contributions sont bienvenues sous le Developer Certificate of Origin : chaque commit porte
une ligne `Signed-off-by` (`git commit -s`). Voir [CONTRIBUTING.fr.md](CONTRIBUTING.fr.md).

## Licence

Sous [licence Apache, version 2.0](LICENSE) ; attribution dans [NOTICE](NOTICE).
Le nom « Sermofur » n'est pas concédé sous Apache-2.0 (section 6).
