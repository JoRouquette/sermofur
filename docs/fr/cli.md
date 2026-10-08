[English](../cli.md) | Français

# CLI 0.2

`smf [--path DOSSIER] [--json] COMMANDE ...`. Le chemin est un dossier local existant, le
dossier courant par défaut ; il désigne le contexte de travail, jamais un scope arbitraire.

Sortie : avec `--json`, les résultats sont en JSON sur stdout et les erreurs en JSON
`{code, message}` sur stderr uniquement, stdout restant vide en cas d'erreur. Sortie humaine :
`root` en texte, objets en JSON indenté, `doctor` en liste de contrôles. Les codes d'erreur et
les codes de sortie sont stables ; les messages sont des textes anglais courts, susceptibles
d'évoluer.

stdout et stderr sont écrits en UTF-8 sans BOM, quelle que soit la page de code de la console.
La page de code de la console n'est basculée en UTF-8 que si stdout ou stderr atteint la
console ; quand les deux sont redirigés, elle n'est pas touchée. `smf` restaure la page de code
d'origine à la fin normale, sur erreur gérée et sur Ctrl+C : la console de l'appelant n'est pas
modifiée durablement. La restauration n'est pas garantie en cas d'arrêt brutal (processus tué,
fermeture de la fenêtre), ni quand plusieurs `smf` partagent la même console en même temps
(par exemple `smf … | smf …`). Un appelant qui capture la sortie la lit en UTF-8 ; sous
Windows PowerShell, exécuter d'abord
`[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)`, faute de quoi PowerShell
décode la sortie capturée avec la page de code OEM et altère les accents.
JSON en UTF-8 lisible : les lettres accentuées restent littérales ; `<`, `>`, `&` et l'accent
grave restent échappés, si bien qu'un texte ne peut pas fermer le bloc JSON d'une projection.

| Commande | Résultat |
|---|---|
| init | Instance neuve, ou même identité si elle existe déjà ; pas d'instance imbriquée |
| root | Instance la plus proche en remontant (dossier `.sermofur` qui contient `instance.json`) |
| status | Identité, scope, comptes visibles, mode bootstrap |
| doctor | Intégrité, schéma, clés étrangères, scopes, mappings, chevauchements, cohérence, projections ; sans mutation |
| scope current | Scope courant et ancêtres visibles |
| scope list / tree | Scopes visibles uniquement (tree = list en 0.1) |
| scope add ID KIND PARENT CHEMIN_RELATIF | Enfant direct du scope courant ; parent immédiat typé |
| claim add TEXTE --origin user\|llm | Claim proposé, confiance calculée à la lecture |
| claim list / show ID | Objets visibles / le claim avec ses preuves et son historique |
| claim invalidate ID --reason TEXTE | Invalidation auditable d'un claim du scope courant |
| evidence add CLAIM_ID KIND REFERENCE --lineage ORIGINE --origin user\|llm | Preuve déclarée, même scope que le claim |
| evidence list / show ID | Preuves visibles |
| retex add --event TEXTE --impact TEXTE --next TEXTE --origin user\|llm | RETEX brouillon, aucun apprentissage implicite |
| retex list / show ID | RETEX visibles |
| export | Reconstruit les projections des objets visibles |
| migrate | Format d'instance 1 (0.1) → 2 : sauvegarde, puis migration en une transaction ; rien à faire en format 2 |
| source add FICHIER --origin user\|llm | Source déclarée du scope courant : hachée, indexée, idempotente |
| source list / show ID | Sources visibles / la source avec son historique |
| source reindex [ID] | Relit les sources du scope courant : unchanged, modified, missing, unreadable, rejected, restored, ou skipped quand une autre commande a modifié la source entre-temps |
| index rebuild | Reconstruit l'index plein texte de l'instance en une seule transaction d'écriture et relit toutes les sources (mêmes issues que `source reindex`, historique sous l'acteur système `sermofur`) ; les autres commandes attendent au plus 5 s, puis échouent en `storage_busy` (code 3) et sont à relancer ; la sortie `{indexed, changedSources}` ne compte que les objets visibles |
| recall QUESTION [--limit 1-3] | Au plus 3 résultats expliqués parmi claims, RETEX et passages de sources visibles |
| challenge CLAIM_ID / challenge --text TEXTE | Contradictions, sources modifiées, statut, date de revue, claims proches à confronter ; n'écrit rien |

L'usage affiché par `smf --help` est en anglais (`TEXT`, `RELATIVE_PATH`, `ORIGIN`…).

`--origin user|llm` est **obligatoire** sur `claim add`, `evidence add`, `retex add` et
`source add` — pas sur `scope add` —, sans valeur par défaut : son absence donne
`invalid_arguments` (exit 1) et rien n'est écrit. L'origine reste déclarative tant que le canal
host (daemon/MCP) ne la fixe pas.
Autres options de `add` : `--actor` (défaut `local-user`), `--key` pour l'idempotence (pas sur
`source add`, idempotent par chemin).
Claim : `--category episodic|semantic|procedural|preferences|decisions` ;
`--volatility stable|evolving|volatile`.
Types de preuve : `execution`, `source_code`, `authoritative_documentation`, `project_decision`,
`local_documentation`, `human_observation`, `user_assertion`, `llm_assertion`.
Un claim `preferences` ou `decisions` exige `--origin user`. Une preuve `execution` déclarée
reste low dans cette version. `evidence add` accepte aussi `--contradicts` (preuve contre le
claim, option sans valeur) et `--source SOURCE_ID` (une source visible et indexée ; son empreinte à
cet instant est conservée avec la preuve). Pas d'argument `--scope` : le scope vient du chemin. Options
inconnues refusées avant toute mutation. Pas encore de `--quiet`/`--verbose`.

Bornes de la ligne de commande : 128 arguments au plus, chacun d'au plus 16 384 caractères et
sans caractère NUL. Elles sont contrôlées avant toute commande, donc avant tout service et toute
écriture ; un dépassement donne `invalid_arguments` (exit 1). Le message, en anglais, écrit
« 16,384 ».

`--help` est reconnu n'importe où **avant** `--` et affiche l'usage (exit 0) avant toute
recherche d'instance et avant le contrôle des bornes ; après `--`, c'est un argument positionnel
comme un autre.
`--` termine les options : tout argument qui suit est positionnel, ce qui permet un texte
commençant par `--` (`smf claim add --origin user -- "--texte"`).
Une valeur d'option ne peut pas commencer par `--` : `--actor --x` donne `invalid_arguments`
(exit 1). Seul un argument positionnel placé après `--` peut commencer par deux tirets.

## init

| Code | Exit | Cas |
|---|---|---|
| `invalid_path` | 1 | Le dossier n'existe pas |
| `nested_instance` | 4 | L'entrée `.sermofur` la plus proche au-dessus est une instance valide |
| `invalid_instance` | 3 | L'entrée `.sermofur` la plus proche, dans le dossier cible ou au-dessus, n'est pas une instance valide (étrangère, endommagée ou illisible) ; elle n'est pas touchée |
| `unsafe_path` | 4 | Chemin réseau (UNC ou lecteur réseau mappé), lien ou jonction, y compris un lien `.sermofur` |

La découverte échoue fermé ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)) : elle
s'arrête à l'entrée `.sermofur` la plus proche, quelle qu'elle soit. Si cette entrée n'est pas
une instance valide, toute commande sauf `doctor` échoue en `invalid_instance` (exit 3) pour une
entrée étrangère (un fichier, ou un dossier qui ne contient ni `instance.json`, ni `memory.db`, ni
`records/`), une **instance endommagée** (`memory.db` ou `records/` sans `instance.json`) ou une
entrée illisible (attributs ou listage impossibles à lire). Un lien ou une jonction, même
pendant, est refusé en `unsafe_path` (exit 4) par toutes les commandes, `doctor` compris. Rien
n'est jamais écrit dans une instance plus haut. Pour une instance endommagée, lancer
`smf doctor` depuis ce dossier, puis restaurer `instance.json` depuis une sauvegarde ; pour une
entrée étrangère, la renommer ou la déplacer. `doctor` nomme le cas dans son contrôle
`instance` : `invalid_instance: foreign`, `invalid_instance: damaged`,
`invalid_instance: unreadable` ou `invalid_instance: foreign_owner`.

Une entrée `.sermofur` qui appartient à un autre compte utilisateur n'est jamais lue : toute
commande sauf `doctor` échoue en `foreign_owner` (exit 4) ([ADR 0014](adr/0014-instance-owner.md)).
Un `instance.json` de plus de 64 Kio est une instance endommagée (`invalid_instance`, exit 3).

## migrate

L'outil 0.2 lit les instances au format 2. Sur une instance au format 1 (créée par la 0.1), toute
commande sauf `migrate`, `doctor` et `root` échoue en `migration_required` (exit 3) sans rien
modifier ; `doctor` signale `migration_required`. `smf migrate` vérifie la cohérence de la base,
la sauvegarde par l'API de sauvegarde SQLite dans
`.sermofur/backups/memory-v1-<heure UTC>-<id>.db`, la migre en une transaction (objets,
historique, clés d'idempotence et projections inchangés ; claims et RETEX indexés), puis remplace
`instance.json`. S'il est interrompu, le relancer : il reprend où il s'est arrêté. Sortie :
`{migrated, from, to, backup}` ; sur une instance au format 2, `migrated: false`
([ADR 0012](adr/0012-instance-format-2.md)).

## Sources

`source add FICHIER` déclare un fichier texte local comme source du scope courant. Un `FICHIER` relatif
l'est au dossier de contexte (`--path`, le dossier courant par défaut). Le fichier doit être dans
le dossier du scope courant (la racine de l'instance pour le workspace) et hors du dossier de tout
scope plus précis : le fichier d'un client s'ajoute depuis le dossier de ce client, pour qu'il ne
devienne jamais visible de ses frères. Le fichier est lu une fois, 1 Mio au plus, en UTF-8 strict
(un BOM est accepté) ; son empreinte SHA-256 couvre exactement les octets indexés. Son texte est
découpé en passages d'au plus 2 000 caractères. Ajouter de nouveau le même fichier rend la même
source.

`source reindex` ne relit que les sources déclarées du scope courant ; aucun autre fichier n'est
jamais lu, et `recall`/`challenge` ne lisent aucun fichier. L'empreinte précédente reste dans
l'historique. Quand `scope add` crée un scope dont le dossier contient des sources d'un ancêtre,
ces sources passent au nouveau scope dans la même transaction (historique « rescoped »), pour ne
jamais rester visibles de ses frères ; `doctor` signale une source rattachée à un scope plus large
que son fichier (`source_scopes`). Une source est enregistrée sous le nom de son fichier sur le disque : sur un système de fichiers
insensible à la casse (Windows, macOS par défaut), `Doc.md` et `doc.md` sont une seule source. Une source absente, illisible ou refusée sort de l'index, son enregistrement est
conservé.

| Code | Exit | Cas |
|---|---|---|
| `source_rejected` | 1 | Vide, binaire (octet NUL, UTF-8 invalide, fichier spécial) ou de plus de 1 Mio ; le message donne la raison |
| `invalid_path` | 1 | Fichier absent, dossier ou fichier illisible |
| `scope_boundary` | 4 | Hors de l'instance, hors du dossier du scope courant, dans un scope plus précis, ou déjà source d'un autre scope |
| `unsafe_path` | 4 | Lien, jonction, chemin réseau, ou fichier de `.sermofur` |
| `source_unavailable` | 1 | `evidence add --source` sur une source absente, illisible ou refusée |

## recall et challenge

`recall` traite la question comme du texte (aucune syntaxe de requête), sans tenir compte de la
casse ni des accents. Il rend au plus 3 résultats, chacun avec son type, son identifiant, son
scope, son statut, sa confiance et ses raisons (claims), l'extrait qui correspond, les termes
trouvés et sa fraîcheur (création, dernière vérification ou date de revue d'un claim, dernière
indexation et empreinte d'une source). Seuls le scope courant et ses ancêtres sont interrogés, et
les statistiques de classement sont calculées sur eux seuls : le contenu d'un autre scope ne
change ni la présence ni l'ordre ([ADR 0013](adr/0013-filtered-ranking.md)). Un claim invalidé ou
remplacé n'est jamais présenté comme applicable : il ne fait que compléter les places restantes,
marqué `applicable: false` ; `excluded` compte les visibles laissés de côté.

`challenge CLAIM_ID` rend des signaux — `contradiction`, `source_changed` (avec l'empreinte
enregistrée et l'empreinte actuelle), `source_unavailable`, `not_applicable`, `review_due` — et au
plus 3 claims proches `toConfront`, jamais qualifiés de contradictions. `challenge --text TEXTE`
fait de même pour un texte qui n'est pas encore un claim. Aucun des deux n'écrit. Un identifiant
d'un autre scope répond comme un identifiant inconnu.

## scope add

| Code | Exit | Cas |
|---|---|---|
| `invalid_arguments` | 1 | Borne de la ligne de commande dépassée (voir plus haut) : refusée avant le service de scopes |
| `invalid_scope` | 1 | Slug invalide (`[a-z][a-z0-9-]{0,63}`), identifiant ou mapping de plus de 16 384 caractères ou contenant NUL (contrôle du service seulement : inatteignable depuis la CLI, qui refuse avant en `invalid_arguments` ; il protège le futur canal daemon/MCP), type inconnu ou incompatible avec le parent, mapping vide, identifiant déjà pris — message identique quel que soit le scope propriétaire, pour ne pas révéler un scope frère |
| `duplicate_mapping` | 1 | Chemin déjà mappé à l'identique par un autre scope (hors repository qui partage le chemin de son project) |
| `invalid_path` | 1 | Dossier mappé inexistant |
| `scope_boundary` | 4 | Parent autre que le scope courant, mapping hors du parent ou de l'instance, ou mapping qui contient ou est contenu dans celui d'une autre branche |
| `unsafe_path` | 4 | Mapping absolu ou avec lecteur, lien, jonction, chemin réseau (UNC ou lecteur réseau mappé) |

Le mapping est stocké relativement à la racine de l'instance avec des barres obliques (`a/b`),
quel que soit le système ; `\` est aussi accepté comme séparateur en entrée et à la lecture
([ADR 0009](adr/0009-portable-mapping-separator.md)).

Revalidation transactionnelle : le mapping candidat et le parent sont contrôlés physiquement
avant l'écriture ; l'identifiant libre, le doublon et le chevauchement sont rejoués sur les
scopes relus sous la transaction `BEGIN IMMEDIATE` de l'insertion. Deux `scope add` concurrents
ne peuvent donc pas enregistrer deux mappings incompatibles : le second est refusé avec le code
ci-dessus et rien n'est écrit. `doctor` applique les mêmes règles (`scope_overlap`, erreur, avec
le seul nombre de paires en conflit) pour détecter un chevauchement entré hors CLI.

Un dossier mappé supprimé n'empêche pas les autres contextes de fonctionner : seul le mapping
retenu pour le chemin courant est contrôlé physiquement. `doctor` le signale en warning
`scope_mappings`, avec le seul nombre de mappings absents.

| Exit | Signification |
|---|---|
| 0 | Succès, aide, ou doctor sain avec warnings |
| 1 | Entrée invalide / not_found / conflit idempotent / mapping dupliqué |
| 2 | Instance absente |
| 3 | Stockage/version/permissions/projection à reconstruire, entrée `.sermofur` invalide (étrangère, endommagée, illisible), migration requise |
| 4 | Frontière scope/chemin, instance imbriquée, entrée d'un autre compte |
| 5 | Doctor unhealthy |

La CLI appelle directement Application. runtime/mcp/config sont au backlog, jamais des commandes
vides qui annoncent un succès.
