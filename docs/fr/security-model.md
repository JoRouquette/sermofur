[English](../security-model.md) | Français

# Modèle de sécurité

Local d'abord : ni écoute réseau, ni télémétrie, ni synchronisation cloud, ni envoi à un LLM.

Le daemon ([daemon.md](daemon.md)) tourne sous votre compte et n'écoute sur aucun port : un pipe
nommé réservé à votre compte sous Windows, ailleurs un socket Unix `0600` dans un dossier `0700`,
dont le serveur contrôle l'identifiant utilisateur de chaque pair. Un point de connexion ou un
dossier d'un autre compte, ou ouvert à d'autres, est refusé (`foreign_endpoint`) et la CLI
s'exécute directement. Le daemon ne sert que les instances que vous avez enregistrées : une
requête pour tout autre chemin est refusée sans le lire. Le scope d'une session vient du dossier
de lancement du client et aucune requête ne peut le changer. Les requêtes sont bornées (256 Kio,
60 s) ; le journal n'enregistre aucun argument ni aucune sortie. Tout processus de votre compte
peut utiliser ou arrêter votre daemon, comme il peut déjà lire votre instance.

Le serveur MCP ([mcp-integration.md](mcp-integration.md)) ne contient aucun moteur : ses outils
s'exécutent via le daemon dans le scope du dossier du projet. Ses schémas sont fermés et ne
prennent ni chemin, ni scope, ni instance, ni origine, ni acteur ; toute écriture est enregistrée
avec l'origine `llm` et le host comme acteur, si bien qu'un modèle ne peut pas faire passer ses
affirmations pour les vôtres, et un identifiant d'un scope frère répond comme un identifiant
inconnu. Il n'offre ni SQL, ni accès aux fichiers, ni ressource, ni prompt.

Un seul utilisateur OS. Les scopes protègent les opérations du produit ; le même utilisateur
OS peut accéder à ses fichiers SQLite et Markdown. Ni chiffrement, ni ACL multi-utilisateur, ni
bac à sable contre un code hostile.

Isolation dans Application et requêtes filtrées ; les IDs ne donnent aucune permission ; preuves
dans le même scope. Le service recalcule les ancêtres et ne croit pas une visibilité forgée par
un appelant. SQL paramétré ; entrées bornées ; options inconnues refusées ; erreurs sans charge
métier. Les références de preuves ne sont jamais ouvertes. Un fichier source n'est lu que lorsque
l'utilisateur l'ajoute ou le réindexe explicitement : dans le dossier du scope courant et hors des
scopes plus précis, sans lien, jonction, chemin réseau, fichier spécial ni fichier de `.sermofur`,
1 Mio au plus, lu une seule fois. Recall et challenge ne lisent aucun fichier. Le type d'une source est contrôlé par son chemin avant
son ouverture : un processus qui peut écrire dans le dossier peut substituer le fichier entre les
deux, les sources doivent donc vivre dans des dossiers où vous seul pouvez écrire
([ADR 0014](adr/0014-instance-owner.md)). La question est
découpée en termes par le tokenizer de l'index puis comparée au seul vocabulaire de l'index :
aucune syntaxe de requête
n'atteint le moteur, et les statistiques de classement ne viennent que des scopes visibles
([ADR 0013](adr/0013-filtered-ranking.md)).
Points d'analyse (reparse points) et chemins UNC refusés pour le stockage et les mappings ; init
utilise des fichiers temporaires adjacents et un renommage. Toute entrée `.sermofur` qui n'est
pas une instance valide (étrangère, endommagée, illisible, ou un lien) n'est jamais adoptée ni
écrasée et arrête la découverte, pour qu'aucune commande n'écrive jamais dans une instance
ancêtre ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)).
Limites explicites : pas de protection contre le remplacement concurrent d'un chemin par un
processus malveillant du même utilisateur (envisager une résolution par handles avant
d'autoriser les liens). La découverte refuse une entrée `.sermofur` détenue par un autre compte
(`foreign_owner`, [ADR 0014](adr/0014-instance-owner.md)) : l'entrée d'un autre utilisateur ne peut
plus recevoir votre mémoire ni vous fournir la sienne ; elle peut encore bloquer la découverte et
`init` en dessous (par défaut la racine d'un lecteur Windows, les dossiers créés à cette racine, un
volume NTFS de données, un support FAT/exFAT, ou `/tmp`), et une instance gardée dans un dossier
où d'autres peuvent écrire reste lisible et modifiable par eux. Sur une machine partagée, garder
votre instance dans un dossier où vous seul pouvez écrire et supprimer toute entrée qu'un autre
utilisateur a créée au-dessus.

Aucun journal technique ne contient les textes cognitifs. stdout est une sortie explicite vers le
demandeur, à traiter comme donnée privée. `.sermofur` doit rester hors d'un Git autosynchronisé.
doctor ne répare ni ne migre (`smf migrate` est explicite et sauvegarde d'abord) ; il donne un diagnostic structurel global sans révéler les charges
métier.
Audit NuGet bloquant ; versions épinglées et fichiers de verrouillage commités ; le runtime SQLite
natif est à contrôler avant toute activation de WAL. Sources et modèles réseau seulement par une
future installation explicite.
