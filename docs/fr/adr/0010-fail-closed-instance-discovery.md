[English](../../adr/0010-fail-closed-instance-discovery.md) | Français

# ADR 0010-fail-closed-instance-discovery — Découverte d'instance en échec fermé

Date : 2026-10-07. Statut : accepté, implémenté en 0.1. Complète
l'[ADR 0001](0001-storage.md).

## Contexte
Les commandes trouvent leur instance en remontant depuis le dossier de travail jusqu'à l'entrée
`.sermofur` la plus proche. Cette entrée n'est pas toujours une instance valide : dossier copié à
la main, données d'un autre outil, instance endommagée, illisible ou remplacée par un lien.
Ignorer les entrées qui ne ressemblent pas à des données Sermofur laisse des trous (dossier
illisible, dossier vidé, lien pendant) où une commande écrirait en silence dans une instance
ancêtre, c'est-à-dire dans une autre mémoire.

## Décision
La découverte s'arrête à l'entrée `.sermofur` la plus proche, quelle qu'elle soit. Une instance
valide (un dossier qui contient `instance.json`) est utilisée. Toute autre entrée échoue fermé :

| Entrée | Résultat |
|---|---|
| fichier, ou dossier sans aucun artefact Sermofur | `invalid_instance` (3), « la renommer ou la déplacer » |
| `memory.db` ou `records/` sans `instance.json` | `invalid_instance` (3), instance endommagée |
| entrée dont les attributs ou le listage ne peuvent pas être lus, ou dont la présence ne peut pas être déterminée | `invalid_instance` (3), entrée illisible |
| lien ou jonction, pendant ou non | `unsafe_path` (4) |

L'entrée est inspectée sans suivre les liens. `doctor` est la seule commande qui s'exécute sur une
entrée invalide (sauf un lien), pour la signaler dans son contrôle `instance`. `init` n'adopte ni
n'écrase jamais une entrée. Dans une instance listable, un `instance.json` illisible est une
erreur de stockage, pas une entrée illisible : toute commande qui lit la configuration (toutes
sauf `root` et `doctor`) échoue en `invalid_storage_or_path` (exit 3), et `doctor` signale
`invalid_storage` dans son contrôle `instance` (exit 5).

## Alternatives
Ignorer les entrées étrangères et ne s'arrêter que sur une instance endommagée : écarté, la
classification peut être trompée par des permissions ou un dossier vidé, et l'échec prend la
forme d'une écriture silencieuse ailleurs. Se fier au seul nom du marqueur : un nom distinctif
rend une entrée étrangère peu probable, mais ne supprime pas les cas endommagé et illisible.

## Conséquences
Une entrée `.sermofur` qui appartient à un autre outil bloque Sermofur en dessous d'elle, avec un
message explicite ; l'utilisateur la renomme ou la déplace, ou travaille depuis un autre dossier.
Aucune commande ne retombe jamais sur une instance ancêtre.

La découverte remonte jusqu'à la racine du volume et ne vérifie pas le propriétaire de l'entrée
trouvée. Sermofur suppose un seul utilisateur du système. Sur une machine partagée, un autre
utilisateur peut créer une entrée `.sermofur` dans tout dossier situé au-dessus de votre dossier
de travail où il peut écrire. Sous Windows, tout utilisateur authentifié peut par défaut créer un
dossier à la racine d'un lecteur (`C:\`), et les dossiers créés à cette racine, sur un volume NTFS
de données ou sur un support FAT/exFAT sont inscriptibles par tout utilisateur authentifié. Sous
Unix, n'importe qui peut créer une entrée dans `/tmp`. Une telle entrée bloque la découverte et
`init` en dessous d'elle, reçoit la mémoire que vous écrivez depuis un dossier qu'aucune de vos
instances ne contient, ou vous fournit des claims et des RETEX écrits par l'autre utilisateur ;
une instance gardée dans un tel dossier peut aussi être lue ou modifiée par lui. Comme `init` et
toute commande lancée hors de vos instances remontent jusqu'à la racine du lecteur, garder vos
fichiers dans votre profil ne suffit pas à lui seul. Tant qu'aucun contrôle du propriétaire
n'existe, sur une machine partagée :

- garder votre instance sous un dossier où vous seul pouvez écrire, comme votre profil Windows
  ou votre home Unix ;
- ne lancer `smf` que depuis l'intérieur de cette instance, et vérifier avec `smf root` que la
  racine trouvée est la vôtre avant d'écrire ;
- si `init` signale une entrée à la racine du lecteur, vérifier à qui elle appartient et la
  supprimer avant de relancer `init`.
