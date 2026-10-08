[English](../memory-model.md) | Français

# Modèle mémoire

Un claim factuel commence `proposed` ; un RETEX commence `draft`. Catégories :
episodic / semantic / procedural / preferences / decisions ; volatilité : stable / evolving /
volatile. Les décisions et préférences expriment une volonté. Les faits utilisateur et les
sorties LLM restent low sans support plus fort.

La confiance est rendue avec ses raisons : low sans support fiable ; medium pour une
observation, une documentation locale ou une décision de projet ; high pour du code source ou une
documentation autoritative déclarés ; verified est réservé à une future exécution collectée et
contrôlée. Ce niveau décrit le support, jamais une probabilité objective. Les raisons sont des
textes en anglais.
Plusieurs preuves de même lignée comptent pour une origine ; la confiance ne se multiplie pas.
Les références de preuves sont déclaratives : aucune référence n'est ouverte.

Une preuve soutient (défaut) ou contredit son claim (`--contradicts`). Le niveau est calculé sur
les seules preuves qui soutiennent. Une preuve contraire d'origine non LLM plafonne le niveau à
medium, avec la raison « Contested by N independent origin(s) » (lignées distinctes) ; une
contradiction d'un LLM ajoute une raison et ne réfute rien. Une preuve peut citer une source
déclarée (`--source`) : l'empreinte de la source à cet instant est conservée, pour que
`challenge` signale une source modifiée ou disparue depuis.

Le recall classe les claims, RETEX et passages de sources visibles par BM25 (k1 = 1,2, b = 0,75)
calculé sur les seuls passages visibles, garde le meilleur passage par objet, pondère un claim par
sa confiance (low 1,0 ; medium 1,1 ; high 1,2) et départage par date de création puis
identifiant ([ADR 0013](adr/0013-filtered-ranking.md)). Au plus 3 résultats ; les claims invalidés
ou remplacés ne sont jamais applicables et ne font que compléter les places restantes. Le
challenge rend des signaux déclarés ou mesurables et des claims proches à confronter ; il ne
change aucun statut.

`claim show` rend le claim, les preuves visibles, la confiance et l'historique. L'invalidation
conserve l'état précédent et le nouvel état, la raison, l'acteur et l'horodatage. Une clé
idempotente identique conserve l'ID, mais un contenu différent échoue. Sur une erreur
`projection_pending` après commit, ne pas recréer sans clé ; inspecter l'ID fourni puis lancer
`export`. L'export exige le contexte de l'objet ou l'un de ses descendants.

À construire : réflexion, validation/rejet de RETEX, apprentissage, recherche de doublons,
similarité sémantique, supersession et revalidation, consolidation, oubli contrôlé, fraîcheur calculée
par politique. Les champs `lastVerified` et `reviewAfter` sont persistés ; l'édition par la CLI
et les collecteurs ne sont pas livrés.
