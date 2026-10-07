[English](CONTRIBUTING.md) | Français

# Contribuer à Sermofur

Merci de votre intérêt. Les issues et les pull requests sont bienvenues.

## Developer Certificate of Origin

Les contributions sont acceptées sous le [Developer Certificate of Origin 1.1](https://developercertificate.org/)
(DCO). En ajoutant la ligne `Signed-off-by` (sign-off) à un commit, vous certifiez avoir écrit
la modification ou avoir le droit de la soumettre sous la licence du projet, la
[licence Apache, version 2.0](LICENSE).

Chaque commit porte une ligne `Signed-off-by` avec votre vrai nom et votre adresse :

```text
Signed-off-by: Jeanne Martin <jeanne.martin@example.com>
```

`git commit -s` l'ajoute pour vous. Pour l'ajouter à des commits déjà faits sur votre branche,
`git rebase --signoff <base>` puis un push forcé de la branche. **Une pull request qui contient un
commit sans ligne `Signed-off-by` n'est pas fusionnée.** Une signature GPG ou SSH n'est ni exigée
ni suffisante : seule la ligne `Signed-off-by` compte.

## Prérequis

- [SDK .NET 10](https://dotnet.microsoft.com/download) (voir `global.json`).
- [CSharpier](https://csharpier.com/) pour le formatage.

## Construire, tester, formater

```powershell
dotnet restore --locked-mode
dotnet build
dotnet test
csharpier format .
```

Lancer `csharpier check .` avant d'ouvrir une pull request. Les tests de processus lancent la CLI
construite : ne pas utiliser `dotnet test --no-build` après une modification du code. Plus :
[environnement de développement](docs/fr/developer-setup.md) et [vérification](docs/fr/verification.md).

## Style de code

Les analyseurs imposent les règles principales à la compilation, avertissements traités comme
des erreurs :

- types explicites plutôt que `var` (`var` seulement pour un type anonyme) ;
- accolades sur tout bloc, même d'une ligne ;
- types référence nullables activés, aucun avertissement masqué par `!` sans raison.

Les couches restent séparées : Domain sans dépendance externe, Application porte les règles,
Infrastructure implémente les ports, la CLI compose. Code, identifiants, commentaires, messages de
la CLI et messages de commit sont en anglais. Toute modification de comportement vient avec ses
tests ; une correction de bug commence par le test qui le reproduit.

Ne pas changer les codes d'erreur, les codes de sortie, les noms de champs JSON ni le format
persistant sans discussion préalable dans une issue : ce sont des contrats.

## Commits et pull requests

Suivre les [Conventional Commits](https://www.conventionalcommits.org/) : `feat:`, `fix:`, `docs:`,
`test:`, `refactor:`, `chore:`… Un sujet par pull request, avec la description de la
modification et de la façon dont elle a été vérifiée.

## Documentation

L'anglais est la langue de référence ; la traduction française vit dans `README.fr.md` et
`docs/fr/`. Si possible, mettre à jour les deux ; sinon, le signaler dans la pull request et la
page française sera mise à jour ensuite.

## Spécifications

Le mainteneur tient les spécifications du produit dans un atelier Spec Kit hors de ce dépôt. Les
contributeurs n'ont pas à l'utiliser : une issue et une pull request claire suffisent. Les
décisions durables sont consignées en [ADR](docs/fr/adr/).
