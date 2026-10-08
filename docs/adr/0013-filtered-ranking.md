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
- FTS5 only finds candidates. The query restricts rows to visible scopes in the same statement.
- Sermofur computes BM25 itself (k1 = 1.2, b = 0.75) from the per-passage term frequencies of the
  `fts5vocab` instance table, with statistics computed on visible passages only.
- The query is tokenized by the index tokenizer itself (a temporary FTS5 table of the same
  configuration), and every term is sent as a quoted string: no query syntax from the user
  reaches the engine.
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
