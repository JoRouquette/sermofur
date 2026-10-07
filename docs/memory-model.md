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
multiplied. All evidence references in 0.1 are declarative: no source content is read or
attested.

`claim show` returns the claim, the visible evidence, the confidence and the history.
Invalidation keeps the previous and new state, the reason, the actor and the timestamp. An
identical idempotency key keeps the ID, but different content fails. On a `projection_pending`
error after commit, do not recreate without a key; inspect the given ID then run `export`.
Export requires the object's context or one of its descendants.

To be built: reflect, RETEX validation/rejection, learn, duplicate search, contradictions,
supersession and revalidation, consolidation, controlled forgetting, freshness computed by
policy. The `lastVerified` and `reviewAfter` fields are persisted; CLI editing and collectors
are not delivered.
