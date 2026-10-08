English | [Français](../fr/adr/0013-filtered-ranking.md)

# ADR 0013-filtered-ranking — Visibility before ranking, statistics included

Date: 2026-10-08. Status: accepted, implemented in 0.2. Completes
[ADR 0002](0002-scopes.md).

## Context
Recall must return at most 3 results from the current scope and its ancestors. Filtering the
results after ranking is not enough: the `bm25()` function of FTS5 computes its statistics
(document count, document frequency of each term, average length) on the whole table, so the
content of a sibling scope would change the order of visible results and leak through it.

## Decision
- Sermofur computes BM25 itself (k1 = 1.2, b = 0.75). The per-passage term frequencies come
  from the `fts5vocab` instance table, queried with the terms bound as parameters; the passages
  of scopes that are not visible are dropped before anything is computed, and the statistics
  (document count, document frequency, average length) come from visible passages only. The
  `bm25()` function and the `MATCH` operator are not used.
- The query is tokenized by the index tokenizer itself (a temporary FTS5 table of the same
  configuration): no query syntax from the user reaches the engine.
- The frequency query reads the postings of the whole instance before the visibility filter: it
  leaks nothing, but its cost grows with the content of other scopes.
- Each object keeps its best passage. A claim's relevance is weighted by its confidence (low 1.0,
  medium 1.1, high 1.2); invalidated and superseded claims are not applicable and only fill
  remaining places, marked as such. Ties are broken by creation date, then identifier.

## Alternatives
`bm25()` with a scope filter: the order depends on invisible content. One FTS5 table per scope:
scores of ancestors would not be comparable. A reimplementation of the tokenizer: SQLite's
`unicode61` uses Unicode 6.1 tables and would diverge on recent characters.

## Consequences
A test proves that adding content to a sibling scope changes neither the presence, the order nor
the score of visible results. Ranking costs one grouped query on the vocabulary table; the
measure is published in [verification](../verification.md).
