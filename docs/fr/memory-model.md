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
Toutes les références de preuves de la 0.1 sont déclaratives : aucun contenu source n'est lu ni
attesté.

`claim show` rend le claim, les preuves visibles, la confiance et l'historique. L'invalidation
conserve l'état précédent et le nouvel état, la raison, l'acteur et l'horodatage. Une clé
idempotente identique conserve l'ID, mais un contenu différent échoue. Sur une erreur
`projection_pending` après commit, ne pas recréer sans clé ; inspecter l'ID fourni puis lancer
`export`. L'export exige le contexte de l'objet ou l'un de ses descendants.

À construire : réflexion, validation/rejet de RETEX, apprentissage, recherche de doublons,
contradictions, supersession et revalidation, consolidation, oubli contrôlé, fraîcheur calculée
par politique. Les champs `lastVerified` et `reviewAfter` sont persistés ; l'édition par la CLI
et les collecteurs ne sont pas livrés.
