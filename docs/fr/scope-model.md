[English](../scope-model.md) | Français

# Modèle de scopes

workspace → client → project → repository → task. Un seul arbre par instance. IDs en slugs
uniques. La visibilité inclut le scope courant et ses ancêtres seulement ; elle exclut les
descendants, les frères et les autres clients. Le workspace n'est pas un accès administrateur à
la mémoire des clients. doctor vérifie globalement les structures de l'instance, mais ne rend
aucun contenu métier.

Mappings de chemins explicites ; pas de noms de clients déduits par heuristique. Sans mapping, le
contexte est le workspace. Le plus long chemin contenant le contexte gagne. Un project et son
repository enfant peuvent partager un chemin réel ; le repository gagne l'égalité. Des frères ne
partagent pas un mapping.
Non-chevauchement entre branches : un mapping ne peut ni contenir ni être contenu dans le mapping
d'un scope qui n'est ni son ancêtre ni son descendant. Deux clients ne s'imbriquent donc pas, et
le plus long préfixe ne peut jamais faire glisser un contexte d'un client à un autre.
Les liens, jonctions et chemins réseau (UNC comme lecteur réseau mappé sous Windows) sont refusés
dans cette version. Un dossier mappé disparu ne bloque que son propre contexte ; doctor le signale
en warning. Ces règles sont rejouées sous la transaction d'écriture de `scope add`, et doctor les
vérifie sur l'arbre stocké (`scope_overlap`).

Conséquence assumée en 0.1 : un project qui partage le chemin de son repository n'est plus
atteignable comme contexte d'écriture une fois le repository enregistré, puisque le repository
gagne l'égalité. Écrire au niveau du project exige alors un mapping distinct du repository.

Un nouveau mapping est enregistré sous le nom de son dossier sur le disque (casse et normalisation
Unicode, comme `source add` pour les chemins de source) : un scope et les sources de son dossier
se comparent toujours sous la même écriture ; un alias 8.3 de Windows est refusé. `doctor` avertit
(`scope_mappings`) d'un mapping enregistré sous une autre écriture par une version antérieure, car un tel scope peut ne pas contenir les sources de son dossier.

Limite connue : les contrôles de doublon et de chevauchement entre mappings restent lexicaux
(chemins normalisés, sans accès disque) ; la comparaison ignore la casse sous Windows et la
respecte ailleurs. Des mappings enregistrés par une version antérieure sous deux écritures d'un
même dossier peuvent donc y échapper.

Exemple pour un projet mono-dépôt existant, sans créer de dossiers :
```powershell
smf --path C:\work scope add acme client workspace clients/acme
smf --path C:\work\clients\acme scope add billing project acme clients/acme/billing-api
smf --path C:\work\clients\acme\billing-api scope add billing-repo repository billing clients/acme/billing-api
```
Tous les chemins de mapping sont relatifs à la racine de l'instance, même depuis un sous-dossier.
Ils sont stockés avec des barres obliques ([format d'instance](instance-format.md)).
Cette première verticale ne sélectionne pas de task logique hors mapping ; ce contrat viendra
avec les sessions du daemon. Les contextes multiples ou disjoints par scope sont au backlog.

Les preuves vivent avec leur claim. La mutation d'un ancêtre depuis un enfant est refusée ; se
placer dans le contexte parent exprime explicitement l'intention. Promotion inter-client non
implémentée : aucune mémoire brute ne passe de A à B, même via une lignée. Une future
généralisation conservera les sources dans leur scope et n'exposera aux autres clients que le
résultat assaini et validé.
