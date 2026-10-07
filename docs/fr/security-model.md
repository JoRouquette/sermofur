[English](../security-model.md) | Français

# Modèle de sécurité

Local d'abord : ni écoute réseau, ni télémétrie, ni synchronisation cloud, ni envoi à un LLM en
0.1. Un seul utilisateur OS. Les scopes protègent les opérations du produit ; le même utilisateur
OS peut accéder à ses fichiers SQLite et Markdown. Ni chiffrement, ni ACL multi-utilisateur, ni
bac à sable contre un code hostile.

Isolation dans Application et requêtes filtrées ; les IDs ne donnent aucune permission ; preuves
dans le même scope. Le service recalcule les ancêtres et ne croit pas une visibilité forgée par
un appelant. SQL paramétré ; entrées bornées ; options inconnues refusées ; erreurs sans charge
métier. Les sources déclarées ne sont jamais ouvertes : pas de traversée via une référence de
preuve.
Points d'analyse (reparse points) et chemins UNC refusés pour le stockage et les mappings ; init
utilise des fichiers temporaires adjacents et un renommage. Toute entrée `.sermofur` qui n'est pas une
instance valide (étrangère, endommagée, illisible, ou un lien) n'est jamais adoptée ni écrasée et
arrête la découverte, pour qu'aucune commande n'écrive jamais dans une instance ancêtre
([ADR 0010](adr/0010-fail-closed-instance-discovery.md)).
Limite explicite : pas de protection contre le remplacement concurrent d'un chemin par un
processus malveillant du même utilisateur ; envisager une résolution par handles avant
d'autoriser les liens.

Aucun journal technique ne contient les textes cognitifs. stdout est une sortie explicite vers le
demandeur, à traiter comme donnée privée. `.sermofur` doit rester hors d'un Git autosynchronisé.
doctor ne répare ni ne migre ; il donne un diagnostic structurel global sans révéler les charges
métier.
Audit NuGet bloquant ; versions épinglées et fichiers de verrouillage commités ; le runtime SQLite
natif est à contrôler avant toute activation de WAL. Sources et modèles réseau seulement par une
future installation explicite.
