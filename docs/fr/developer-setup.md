[English](../developer-setup.md) | Français

# Environnement de développement

SDK .NET 10 (`global.json` fixe 10.0.201 avec `rollForward: latestFeature`), Git et
[CSharpier](https://csharpier.com/). `NuGet.Config` limite les sources de paquets à nuget.org.
Les dépendances sont épinglées et restaurées depuis les fichiers de verrouillage
(`packages.lock.json`).
Télémétrie de la CLI .NET : définir `DOTNET_CLI_TELEMETRY_OPTOUT=1` pour les outils de
développement si vous le souhaitez. Le runtime Sermofur lui-même ne collecte aucune télémétrie.

```powershell
dotnet restore --locked-mode
dotnet build --no-restore
dotnet test --no-restore
csharpier format .
csharpier check .
```
Lancer `csharpier check .` avant revue et commit. Les types référence nullables sont activés et
les avertissements sont des erreurs ; les règles de style (types explicites, accolades) sont
imposées à la compilation.
Les tests créent des dossiers temporaires distincts et les effacent ; ils ne lisent aucune donnée
utilisateur.
Les tests de processus lancent la CLI construite : ne pas exécuter `dotnet test --no-build` après
une modification.

Le premier schéma transactionnel 0→1 est créé pour une base neuve ; la lecture refuse toute autre
version. Aucune migration d'instance existante n'est implémentée : migration, sauvegarde et tests
précèdent tout schéma v2.
Aucune CI distante n'est encore configurée. Voir [vérification](verification.md) pour ce qui a été
vérifié et sur quelle plateforme.
