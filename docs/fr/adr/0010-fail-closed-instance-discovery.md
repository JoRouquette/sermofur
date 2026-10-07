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
| attributs ou contenu impossibles à lire, y compris quand la présence de l'entrée ne peut pas être déterminée | `invalid_instance` (3), entrée illisible |
| lien ou jonction, pendant ou non | `unsafe_path` (4) |

L'entrée est inspectée sans suivre les liens. `doctor` est la seule commande qui s'exécute sur une
entrée invalide (sauf un lien), pour la signaler dans son contrôle `instance`. `init` n'adopte ni
n'écrase jamais une entrée.

## Alternatives
Ignorer les entrées étrangères et ne s'arrêter que sur une instance endommagée : écarté, la
classification peut être trompée par des permissions ou un dossier vidé, et l'échec prend la
forme d'une écriture silencieuse ailleurs. Se fier au seul nom du marqueur : un nom distinctif
rend une entrée étrangère peu probable, mais ne supprime pas les cas endommagé et illisible.

## Conséquences
Une entrée `.sermofur` qui appartient à un autre outil bloque Sermofur en dessous d'elle, avec un
message explicite ; l'utilisateur la renomme ou la déplace, ou travaille depuis un autre dossier.
Aucune commande ne retombe jamais sur une instance ancêtre.
