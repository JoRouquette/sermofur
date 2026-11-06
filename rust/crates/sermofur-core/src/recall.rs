//! Recall, as `RecallService` of the .NET oracle (`rust/tests/golden/recall.json` is the truth):
//! at most 3 results, scored by BM25 on the visible passages only (ADR 0013), weighted by the
//! confidence of claims, explained by an excerpt, the matched terms and their freshness.
//!
//! Visibility filters the candidates before any statistic is computed, so nothing invisible
//! changes the presence, the order or the explanation of a result. Non-applicable knowledge
//! (invalidated or superseded) only fills the places left.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::Serialize;
use uuid::Uuid;

use crate::confidence::evaluate_confidence;
use crate::error::{Result, SermofurError};
use crate::format::{DateTimeOffset, round6};
use crate::instance::invalid_storage;
use crate::memory::{MemoryContext, validate_text};
use crate::records::{
    ClaimContent, Confidence, ConfidenceLevel, DotNetEnum, EvidenceContent, KnowledgeStatus,
    MemoryRecord, RecordKind, SourceContent, guid_text, read,
};
use crate::search::{RecordSummary, TermHit, query_terms};
use crate::store::Store;

/// The budget of a recall (`RecallService.MaxResults`).
pub const MAX_RESULTS: i32 = 3;
const K1: f64 = 1.2;
const B: f64 = 0.75;
/// Excerpt length, in UTF-16 units.
const EXCERPT_LENGTH: usize = 300;
/// Highest confidence weight: bounds how much a description can raise a score.
const MAX_WEIGHT: f64 = 1.2;

/// How fresh a result is (`Freshness`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Freshness {
    pub created_at: DateTimeOffset,
    pub last_verified: Option<DateTimeOffset>,
    pub review_after: Option<DateTimeOffset>,
    pub indexed_at: Option<DateTimeOffset>,
    pub hash: Option<String>,
    pub source_status: Option<String>,
}

/// One explained result (`RecallResult`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallResult {
    pub kind: &'static str,
    #[serde(serialize_with = "guid_text")]
    pub id: Uuid,
    pub scope_id: Option<String>,
    pub status: String,
    pub applicable: bool,
    pub confidence: Option<Confidence>,
    pub excerpt: String,
    pub matched_terms: Vec<String>,
    pub freshness: Freshness,
    pub score: f64,
    pub path: Option<String>,
}

/// The answer of `recall` (`RecallAnswer`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallAnswer {
    pub query: String,
    pub results: Vec<RecallResult>,
    pub excluded: i32,
}

/// `RecallService.Recall`: the best visible results for `query`, at most `limit` (1 to 3).
/// `now` decides whether a review date has passed.
pub fn recall(
    store: &Store,
    context: &MemoryContext,
    query: &str,
    limit: i32,
    now: DateTimeOffset,
) -> Result<RecallAnswer> {
    let context = context.checked(store)?;
    validate_text(query)?;
    if !(1..=MAX_RESULTS).contains(&limit) {
        return Err(limit_error());
    }
    let (results, excluded) = select(store, &context, query, None, None, limit, true, now)?;
    Ok(RecallAnswer {
        query: query.to_owned(),
        results,
        excluded,
    })
}

/// `RecallService.CloseClaims`: applicable visible claims close to a text, for `challenge`; at
/// most 3, `exclude` left out.
pub fn close_claims(
    store: &Store,
    context: &MemoryContext,
    text: &str,
    exclude: Option<Uuid>,
    now: DateTimeOffset,
) -> Result<Vec<RecallResult>> {
    let context = context.checked(store)?;
    let kinds: &[&str] = &["claim"];
    Ok(select(
        store,
        &context,
        text,
        Some(kinds),
        exclude,
        MAX_RESULTS,
        false,
        now,
    )?
    .0)
}

/// The message of a limit out of range, also used by the command line.
pub fn limit_error() -> SermofurError {
    SermofurError::invalid_arguments("The limit is between 1 and 3.")
}

/// The best passage of a visible object.
struct Candidate {
    document: i64,
    record: RecordSummary,
    relevance: f64,
    terms: Vec<String>,
    applicable: bool,
}

/// `double.CompareTo`; scores are never NaN.
fn compare(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Scores every visible match by BM25 on visible statistics, then describes only what is shown:
/// confidence weights (at most x1.2) are computed in order of relevance until no remaining
/// candidate can enter the top. Non-applicable knowledge fills the places left when `fill` is
/// set. Returns the results and how many non-applicable matches were left out.
#[allow(clippy::too_many_arguments)]
fn select(
    store: &Store,
    context: &MemoryContext,
    query: &str,
    kinds: Option<&[&str]>,
    exclude: Option<Uuid>,
    limit: i32,
    fill: bool,
    now: DateTimeOffset,
) -> Result<(Vec<RecallResult>, i32)> {
    let limit = usize::try_from(limit).unwrap_or(0);
    let visible = &context.visible;
    let terms = query_terms(store, query)?;
    if terms.is_empty() {
        return Err(SermofurError::new(
            "invalid_input",
            "The question holds no searchable term.",
            1,
        ));
    }
    let hits = store.hits(&terms, visible)?;
    if hits.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let corpus = store.corpus(visible)?;
    let mut documents_of: HashMap<&str, HashSet<i64>> = HashMap::new();
    for hit in &hits {
        documents_of
            .entry(hit.term.as_str())
            .or_default()
            .insert(hit.document);
    }
    let idf: HashMap<&str, f64> = documents_of
        .into_iter()
        .map(|(term, documents)| {
            let documents = documents.len() as f64;
            let n = corpus.documents as f64;
            (term, (1.0 + (n - documents + 0.5) / (documents + 0.5)).ln())
        })
        .collect();
    let records: HashMap<Uuid, RecordSummary> = store
        .read_summaries(visible)?
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    let average_length = corpus.average_length.max(1.0);

    // Passages in order of first appearance (`GroupBy`), then the best passage of each object.
    let mut passages: Vec<(i64, Vec<&TermHit>)> = Vec::new();
    let mut passage_at: HashMap<i64, usize> = HashMap::new();
    for hit in hits.iter().filter(|hit| {
        kinds.is_none_or(|kinds| kinds.contains(&hit.kind.as_str()))
            && Some(hit.object_id) != exclude
            && records.contains_key(&hit.object_id)
    }) {
        let at = *passage_at.entry(hit.document).or_insert_with(|| {
            passages.push((hit.document, Vec::new()));
            passages.len() - 1
        });
        passages[at].1.push(hit);
    }
    let mut best: Vec<Candidate> = Vec::new();
    let mut best_at: HashMap<Uuid, usize> = HashMap::new();
    for (document, group) in passages {
        let first = group[0];
        let norm = K1 * (1.0 - B + B * f64::from(first.length) / average_length);
        let mut relevance = 0.0;
        for hit in &group {
            let frequency = f64::from(hit.frequency);
            relevance += idf[hit.term.as_str()] * frequency * (K1 + 1.0) / (frequency + norm);
        }
        let record = records[&first.object_id];
        let candidate = Candidate {
            document,
            record,
            relevance,
            terms: group.iter().map(|hit| hit.term.clone()).collect(),
            applicable: !matches!(
                record.status,
                KnowledgeStatus::Invalidated | KnowledgeStatus::Superseded
            ),
        };
        match best_at.get(&record.id) {
            Some(&at) => {
                let kept = &mut best[at];
                let better = match compare(candidate.relevance, kept.relevance) {
                    Ordering::Greater => true,
                    Ordering::Equal => candidate.document < kept.document,
                    Ordering::Less => false,
                };
                if better {
                    *kept = candidate;
                }
            }
            None => {
                best_at.insert(record.id, best.len());
                best.push(candidate);
            }
        }
    }
    best.sort_by(|a, b| {
        compare(b.relevance, a.relevance)
            .then_with(|| {
                b.record
                    .created_at
                    .utc_ticks()
                    .cmp(&a.record.created_at.utc_ticks())
            })
            .then_with(|| a.record.id.cmp(&b.record.id))
    });

    // Evidence is parsed once; a claim without evidence is low (weight 1).
    let mut evidence: HashMap<Uuid, Vec<MemoryRecord>> = HashMap::new();
    for record in store.read_records(visible, Some(RecordKind::Evidence))? {
        let json = record.content_json.as_deref().ok_or_else(invalid_storage)?;
        let claim = read::<EvidenceContent>(json, &|_| 0)?.claim_id;
        evidence.entry(claim).or_default().push(record);
    }
    let no_evidence = Vec::new();
    let evidence_of = |id: &Uuid| evidence.get(id).unwrap_or(&no_evidence);

    let mut top: Vec<(&Candidate, f64)> = Vec::new();
    for candidate in best.iter().filter(|c| c.applicable) {
        if top.len() >= limit && candidate.relevance * MAX_WEIGHT < top[limit - 1].1 {
            break;
        }
        let weight = if candidate.record.kind != RecordKind::Claim
            || !evidence.contains_key(&candidate.record.id)
        {
            1.0
        } else {
            let claim = find(store, candidate.record.id, visible)?;
            weight(evaluate_confidence(&claim, evidence_of(&claim.id), now, &|_| 0)?.level)
        };
        let entry = (candidate, candidate.relevance * weight);
        // Bounded insertion: the list never holds more than `limit` entries, in final order.
        let position = top
            .iter()
            .position(|other| ranks(&entry, other) == Ordering::Less)
            .unwrap_or(top.len());
        top.insert(position, entry);
        top.truncate(limit);
    }
    let describe = |candidate: &Candidate| -> Result<RecallResult> {
        let record = find(store, candidate.record.id, visible)?;
        describe(
            store,
            &record,
            evidence_of(&record.id),
            candidate,
            &terms,
            now,
        )
    };
    let mut shown = top
        .iter()
        .map(|(candidate, _)| describe(candidate))
        .collect::<Result<Vec<_>>>()?;
    let not_applicable: Vec<&Candidate> = best.iter().filter(|c| !c.applicable).collect();
    if fill {
        for candidate in not_applicable
            .iter()
            .take(limit.saturating_sub(shown.len()))
        {
            shown.push(describe(candidate)?);
        }
    }
    // A signed `int`, as .NET: an object invalidated between the reads can make it negative.
    let count = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    let excluded =
        count(not_applicable.len()) - count(shown.iter().filter(|r| !r.applicable).count());
    Ok((shown, if fill { excluded } else { 0 }))
}

/// Final order: score, then most recent, then identifier (`Ranks`).
fn ranks(a: &(&Candidate, f64), b: &(&Candidate, f64)) -> Ordering {
    compare(b.1, a.1)
        .then_with(|| {
            b.0.record
                .created_at
                .utc_ticks()
                .cmp(&a.0.record.created_at.utc_ticks())
        })
        .then_with(|| a.0.record.id.cmp(&b.0.record.id))
}

/// The single table of confidence weights, for the ranking and the shown score.
fn weight(level: ConfidenceLevel) -> f64 {
    match level {
        ConfidenceLevel::Medium => 1.1,
        ConfidenceLevel::High | ConfidenceLevel::Verified => MAX_WEIGHT,
        ConfidenceLevel::Low => 1.0,
    }
}

fn find(store: &Store, id: Uuid, visible: &HashSet<String>) -> Result<MemoryRecord> {
    store.find_record(id, visible)?.ok_or_else(|| {
        SermofurError::new("storage_busy", "Object changed during recall; retry.", 3)
    })
}

/// The .NET name of a member in lowercase (`ToString().ToLowerInvariant()`).
fn lower_name<E: DotNetEnum + PartialEq>(member: E) -> String {
    E::NAMES
        .iter()
        .find(|(_, _, m)| *m == member)
        .map(|(name, _, _)| name.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The full description of a shown candidate (`Describe`).
fn describe(
    store: &Store,
    record: &MemoryRecord,
    evidence: &[MemoryRecord],
    candidate: &Candidate,
    terms: &[String],
    now: DateTimeOffset,
) -> Result<RecallResult> {
    let excerpt = excerpt(&store.passage_text(candidate.document)?, &candidate.terms);
    let matched_terms: Vec<String> = terms
        .iter()
        .filter(|term| candidate.terms.contains(term))
        .cloned()
        .collect();
    let json = record.content_json.as_deref().ok_or_else(invalid_storage)?;
    // A missing provenance is a `NullReferenceException` in .NET (corrupt store only).
    let created_at = record
        .provenance
        .as_ref()
        .ok_or_else(invalid_storage)?
        .timestamp;
    let freshness = Freshness {
        created_at,
        last_verified: None,
        review_after: None,
        indexed_at: None,
        hash: None,
        source_status: None,
    };
    let mut result = RecallResult {
        kind: "retex",
        id: record.id,
        scope_id: record.scope_id.clone(),
        status: lower_name(record.status),
        applicable: true,
        confidence: None,
        excerpt,
        matched_terms,
        freshness,
        score: round6(candidate.relevance),
        path: None,
    };
    match record.kind {
        RecordKind::Claim => {
            let claim: ClaimContent = read(json, &|_| 0)?;
            let confidence = evaluate_confidence(record, evidence, now, &|_| 0)?;
            result.kind = "claim";
            result.applicable = !matches!(
                record.status,
                KnowledgeStatus::Invalidated | KnowledgeStatus::Superseded
            );
            result.score = round6(candidate.relevance * weight(confidence.level));
            result.confidence = Some(confidence);
            result.freshness.last_verified = claim.last_verified;
            result.freshness.review_after = claim.review_after;
        }
        RecordKind::Source => {
            let source: SourceContent = read(json, &|_| 0)?;
            result.kind = "source";
            result.freshness.indexed_at = source.indexed_at;
            result.freshness.hash = source.hash;
            result.freshness.source_status = Some(lower_name(source.status));
            result.path = source.relative_path;
        }
        RecordKind::Retex | RecordKind::Evidence => {}
    }
    Ok(result)
}

/// At most 300 UTF-16 units around the first matched term, line breaks flattened, a surrogate
/// pair never cut (`Excerpt`).
fn excerpt(text: &str, matched: &[String]) -> String {
    let flat = text.replace("\r\n", " ").replace('\n', " ");
    let units: Vec<u16> = flat.encode_utf16().collect();
    let length = units.len();
    if length <= EXCERPT_LENGTH {
        return flat;
    }
    let position = matched
        .iter()
        .filter_map(|term| crate::collation::index_of(&flat, term))
        .min()
        .unwrap_or(0);
    let mut start = position
        .saturating_sub(EXCERPT_LENGTH / 3)
        .min(length - EXCERPT_LENGTH);
    let is_low = |unit: u16| (0xDC00..=0xDFFF).contains(&unit);
    let is_high = |unit: u16| (0xD800..=0xDBFF).contains(&unit);
    if start > 0 && is_low(units[start]) {
        start -= 1;
    }
    let mut count = EXCERPT_LENGTH.min(length - start);
    if start + count < length && is_high(units[start + count - 1]) {
        count -= 1;
    }
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(&String::from_utf16_lossy(&units[start..start + count]));
    if start + count < length {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{EXCERPT_LENGTH, excerpt};

    fn units(text: &str) -> usize {
        text.encode_utf16().count()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// The excerpt is the flattened text when short, else a window of 299 or 300 UTF-16
        /// units of it, marked where it is cut, without line breaks or replacement characters.
        #[test]
        fn the_excerpt_is_a_marked_window_of_the_text(
            parts in prop::collection::vec(
                prop::sample::select(vec!["x", " ", "\n", "\r\n", "é", "🙂"]),
                0..900,
            ),
            terms in prop::collection::vec(
                prop::sample::select(vec!["", "x", "é", "🙂", "absent"]),
                0..3,
            ),
        ) {
            let text: String = parts.concat();
            let terms: Vec<String> = terms.into_iter().map(str::to_owned).collect();
            let flat = text.replace("\r\n", " ").replace('\n', " ");
            let shown = excerpt(&text, &terms);
            if units(&flat) <= EXCERPT_LENGTH {
                prop_assert_eq!(shown, flat);
                return Ok(());
            }
            let lead = shown.starts_with('…');
            let trail = shown.ends_with('…');
            let body = shown.strip_prefix('…').unwrap_or(&shown);
            let body = body.strip_suffix('…').unwrap_or(body);
            prop_assert!((EXCERPT_LENGTH - 1..=EXCERPT_LENGTH).contains(&units(body)));
            let clean = !body.contains(['\n', '\r', '\u{FFFD}']);
            prop_assert!(clean);
            prop_assert!(flat.contains(body));
            if !lead {
                prop_assert!(flat.starts_with(body));
            } else {
                prop_assert!(flat.char_indices().skip(1).any(|(i, _)| flat[i..].starts_with(body)));
            }
            if !trail {
                prop_assert!(flat.ends_with(body));
            } else {
                prop_assert!(flat.char_indices().any(|(i, _)| flat[i..].starts_with(body) && i + body.len() < flat.len()));
            }
        }
    }
}
