English | [Français](fr/memory-model.md)

# Memory model

A factual claim starts as `proposed`; a RETEX starts as `draft`. Categories:
episodic / semantic / procedural / preferences / decisions; volatility: stable / evolving /
volatile. Decisions and preferences express an intent. User facts and LLM outputs stay low
without stronger support.

Confidence is returned with its reasons: low without reliable support; medium for an
observation, local documentation or a project decision; high for declared source code or
authoritative documentation; verified is reserved for future collected and controlled execution.
The level describes the support, never an objective probability.
Several pieces of evidence with the same lineage count as one origin; confidence is not
multiplied. Evidence references are declarative: no reference is opened.

Evidence either supports (default) or contradicts its claim (`--contradicts`). The level is
computed from supporting evidence only. Contradicting evidence from a non-LLM origin caps the level
at medium, with the reason "Contested by N independent origin(s)" (distinct lineages); an LLM
contradiction adds a reason and refutes nothing. Evidence can cite a declared source
(`--source`): the hash of the source at that moment is kept, so that `challenge` reports a source
changed or gone since.

Recall ranks visible claims, RETEX and source passages by BM25 (k1 = 1.2, b = 0.75) computed on
visible passages only, keeps the best passage per object, weights a claim by its confidence
(low 1.0, medium 1.1, high 1.2) and breaks ties by creation date then identifier
([ADR 0013](adr/0013-filtered-ranking.md)). At most 3 results; invalidated and superseded claims
are never applicable and only fill remaining places. Challenge returns declared or measurable
signals and close claims to confront; it changes no status.

`claim show` returns the claim, the visible evidence, the confidence and the history.
Invalidation keeps the previous and new state, the reason, the actor and the timestamp. An
identical idempotency key keeps the ID, but different content fails. On a `projection_pending`
error after commit, do not recreate without a key; inspect the given ID then run `export`.
Export requires the object's context or one of its descendants.

To be built: reflect, RETEX validation/rejection, learn, duplicate search, semantic similarity,
supersession and revalidation, consolidation, controlled forgetting, freshness computed by
policy. The `lastVerified` and `reviewAfter` fields are persisted; CLI editing and collectors
are not delivered.
