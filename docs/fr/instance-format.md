[English](../instance-format.md) | Français

# Format d'instance 2

Une instance vit à la racine d'un espace de travail (par exemple le dossier qui contient vos
dépôts). Un autre espace a sa propre `.sermofur`, avec une identité et une base distinctes. Aucune
mémoire machine universelle.

```text
.sermofur/
  instance.json       schemaVersion (2), instanceId, createdAt (UTC) ; 64 Kio au plus
  memory.db           schéma SQL 2 : records (claims, preuves, RETEX, sources), history, scopes,
                      idempotency, metadata, index plein texte search (FTS5)
  records/<GUID>.md   projections lisibles, frontmatter JSON valide YAML
  backups/            sauvegardes de la base faites par smf migrate
```
Un dossier porte une instance seulement s'il contient un **dossier** `.sermofur` avec
`instance.json`. La découverte remonte depuis le dossier de travail et **s'arrête à l'entrée
`.sermofur` la plus proche, quelle qu'elle soit** ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)).
Seule une instance valide est utilisée. Toute autre entrée arrête chaque commande sauf `doctor`
en `invalid_instance` (exit 3) avec un message qui nomme le cas ; `doctor` la signale dans son
contrôle `instance` (exit 5) :

- entrée étrangère : un fichier, ou un dossier qui ne contient ni `instance.json`, ni `memory.db`,
  ni `records/` (dossier vide, données d'un autre outil) — la renommer ou la déplacer ;
- instance endommagée : `memory.db` ou `records/` sans `instance.json` — restaurer `instance.json` ;
- entrée illisible : ses attributs ou son listage ne peuvent pas être lus (permissions), y compris
  quand sa présence même ne peut pas être déterminée ;
- entrée d'un autre compte utilisateur : `foreign_owner` (exit 4), jamais lue
  ([ADR 0014](adr/0014-instance-owner.md)).

Une instance au format 1 (créée par la 0.1) est refusée en `migration_required` (exit 3) par
toutes les commandes sauf `migrate`, `doctor` et `root` ; `smf migrate` la sauvegarde puis la
migre ([ADR 0012](adr/0012-instance-format-2.md)).

Une entrée `.sermofur` qui est un lien ou une jonction, pendante ou non, est refusée en
`unsafe_path` (exit 4) par toutes les commandes, `doctor` compris. La découverte ne continue
jamais vers une instance plus haut : ce serait écrire en silence dans une autre mémoire. `init`
n'adopte ni n'écrase jamais une entrée : il s'arrête sur la même erreur et la laisse intacte.

`init` construit l'instance dans un dossier de staging adjacent, puis la publie par un renommage
qui refuse une destination existante. Sous Unix, un `.sermofur` vide créé en concurrence entre
cette vérification et le renommage n'est pas détecté. `init` ne remplace jamais une instance
incomplète. Un arrêt pendant le staging peut laisser un dossier `.sermofur-init-*` : ne pas le
publier ni l'ingérer manuellement. Aucun nettoyage implicite de données existantes.

Les mappings de scopes sont stockés relativement à la racine de l'instance, **avec des barres
obliques** (`a/b`) quel que soit le système, pour qu'une instance se lise partout de la même
façon. La lecture accepte aussi `\` : un mapping saisi ou stocké avec des barres obliques inverses
se résout à l'identique. Une barre oblique inverse est donc toujours lue
comme un séparateur, y compris dans un nom de dossier sous Unix
([ADR 0009](adr/0009-portable-mapping-separator.md)).

Markdown est une projection, pas une source d'écriture. Toute modification manuelle est signalée
comme divergence ; `export` rétablit la version canonique. Les corrections passent par la CLI et
conservent l'historique. Une source est un fichier texte local déclaré par `source add` ; son enregistrement porte le
chemin relatif, l'empreinte, la taille et la date d'indexation, et son texte ne vit que dans
l'index plein texte, que `index rebuild` reconstruit depuis le registre et les fichiers. La
projection d'une source montre les métadonnées, jamais le contenu.
L'espace de travail doit ignorer `.sermofur` et les dossiers de staging dans Git, pour éviter une
synchronisation de mémoire.
Sauvegarde : arrêter les écrivains puis sauvegarder l'ensemble de `.sermofur` ; ne jamais supposer
qu'une copie de la base active est transactionnellement complète. `smf migrate` fait une copie
cohérente par l'API de sauvegarde SQLite avant de changer le format.
