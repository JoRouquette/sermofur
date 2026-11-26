//! The confidence of a claim, as `MemoryService.EvaluateConfidence` of the .NET oracle
//! (`rust/tests/golden/confidence.json` is the truth): independent origins (lineages) of
//! trusted evidence raise it, LLM assertions never do, an independent contradiction caps it
//! at medium, and each rule leaves its reason.

use crate::error::Result;
use crate::format::DateTimeOffset;
use crate::instance::invalid_storage;
use crate::records::{
    ActorKind, ClaimContent, Confidence, ConfidenceLevel, DotNetEnum, EvidenceContent,
    EvidenceKind, EvidenceRelation, KnowledgeStatus, MemoryCategory, MemoryRecord, read,
};

fn level_index(level: ConfidenceLevel) -> usize {
    ConfidenceLevel::NAMES
        .iter()
        .position(|(_, _, member)| *member == level)
        .unwrap_or(0)
}

fn level_name(level: ConfidenceLevel) -> &'static str {
    ConfidenceLevel::NAMES[level_index(level)].0
}

/// The content of a record; a missing content is the `ArgumentNullException` of
/// `RecordJson.Read(null)`, which the CLI answers with `invalid_storage_or_path`.
fn content_of<T: crate::records::Record>(
    record: &MemoryRecord,
    offset: &dyn Fn(i64) -> i16,
) -> Result<T> {
    let json = record.content_json.as_deref().ok_or_else(invalid_storage)?;
    read(json, offset)
}

/// The origin of a record. .NET dereferences a missing provenance (a corrupt store only) and
/// stops with a `NullReferenceException`; the port refuses the record instead.
fn origin(record: &MemoryRecord) -> Result<ActorKind> {
    record
        .provenance
        .as_ref()
        .map(|p| p.origin)
        .ok_or_else(invalid_storage)
}

/// Record equality of C#: every member, the timestamp compared as an instant.
fn same_record(a: &MemoryRecord, b: &MemoryRecord) -> bool {
    let provenance = match (&a.provenance, &b.provenance) {
        (Some(x), Some(y)) => {
            x.origin == y.origin
                && x.actor == y.actor
                && x.timestamp.utc_ticks() == y.timestamp.utc_ticks()
        }
        (None, None) => true,
        _ => false,
    };
    a.id == b.id
        && a.scope_id == b.scope_id
        && a.kind == b.kind
        && a.status == b.status
        && a.revision == b.revision
        && a.content_json == b.content_json
        && provenance
}

/// The support one piece of evidence gives, by its declared kind.
fn support(kind: EvidenceKind) -> ConfidenceLevel {
    match kind {
        EvidenceKind::SourceCode | EvidenceKind::AuthoritativeDocumentation => {
            ConfidenceLevel::High
        }
        EvidenceKind::ProjectDecision
        | EvidenceKind::LocalDocumentation
        | EvidenceKind::HumanObservation => ConfidenceLevel::Medium,
        _ => ConfidenceLevel::Low,
    }
}

/// `EvaluateConfidence(claim, evidence)` at the instant `now`; `local_offset_minutes` reads
/// a stored date without offset.
pub fn evaluate_confidence(
    claim: &MemoryRecord,
    evidence: &[MemoryRecord],
    now: DateTimeOffset,
    local_offset_minutes: &dyn Fn(i64) -> i16,
) -> Result<Confidence> {
    let low = |reason: &str| Confidence {
        level: ConfidenceLevel::Low,
        reasons: vec![reason.to_owned()],
        independent_origins: 0,
    };
    if matches!(
        claim.status,
        KnowledgeStatus::Invalidated | KnowledgeStatus::Superseded
    ) {
        return Ok(low("Knowledge not applicable: invalidated or superseded."));
    }
    let content: ClaimContent = content_of(claim, local_offset_minutes)?;
    if content
        .review_after
        .is_some_and(|review| review.utc_ticks() <= now.utc_ticks())
    {
        return Ok(low("Review date passed: revalidation required."));
    }

    let mut contradicting: Vec<(&MemoryRecord, EvidenceContent)> = Vec::new();
    for record in evidence {
        let content: EvidenceContent = content_of(record, local_offset_minutes)?;
        if content.relation == EvidenceRelation::Contradicts {
            contradicting.push((record, content));
        }
    }
    // `Except`: the records not contradicting, each kept once, in order.
    let mut remaining: Vec<&MemoryRecord> = Vec::new();
    for record in evidence {
        if !contradicting.iter().any(|(c, _)| same_record(c, record))
            && !remaining.iter().any(|r| same_record(r, record))
        {
            remaining.push(record);
        }
    }
    let mut trusted: Vec<&MemoryRecord> = Vec::new();
    for record in &remaining {
        if origin(record)? != ActorKind::Llm {
            trusted.push(record);
        }
    }
    // `GroupBy` on the lineage (ordinal, `null` a key of its own), in order of first sight.
    let mut origins: Vec<(Option<String>, ConfidenceLevel)> = Vec::new();
    for record in &trusted {
        let content: EvidenceContent = content_of(record, local_offset_minutes)?;
        let given = support(content.kind);
        match origins
            .iter_mut()
            .find(|(key, _)| *key == content.lineage_id)
        {
            Some((_, best)) if level_index(given) > level_index(*best) => *best = given,
            Some(_) => {}
            None => origins.push((content.lineage_id, given)),
        }
    }
    let mut reasons: Vec<String> = Vec::new();
    let mut level = ConfidenceLevel::Low;
    for (key, given) in &origins {
        if level_index(*given) > level_index(level) {
            level = *given;
        }
        reasons.push(format!(
            "Origin {}: support {}; repetitions grouped.",
            key.as_deref().unwrap_or(""),
            level_name(*given)
        ));
    }
    if remaining.len() != trusted.len() {
        reasons.push(
            "LLM assertions excluded from reinforcement, whatever the declared source kind."
                .to_owned(),
        );
    }
    if reasons.is_empty() {
        reasons.push("No independent evidence available.".to_owned());
    }

    let mut contesting: Vec<&Option<String>> = Vec::new();
    let mut llm_contradictions = 0usize;
    for (record, content) in &contradicting {
        if origin(record)? == ActorKind::Llm {
            llm_contradictions += 1;
        } else if !contesting.contains(&&content.lineage_id) {
            contesting.push(&content.lineage_id);
        }
    }
    if !contesting.is_empty() {
        // An independent contradiction forbids a high level (principle I).
        if level_index(level) > level_index(ConfidenceLevel::Medium) {
            level = ConfidenceLevel::Medium;
        }
        reasons.push(format!(
            "Contested by {} independent origin(s).",
            contesting.len()
        ));
    }
    if !contradicting.is_empty() && llm_contradictions == contradicting.len() {
        reasons.push(
            "Contradicted by an LLM assertion: noted, not an independent refutation.".to_owned(),
        );
    } else if llm_contradictions > 0 {
        reasons
            .push("LLM contradictions noted, not counted as independent refutations.".to_owned());
    }

    reasons
        .push("Declared evidence; no controlled execution collector in this version.".to_owned());
    if matches!(
        content.category,
        MemoryCategory::Preferences | MemoryCategory::Decisions
    ) {
        reasons.push(
            "User choice authoritative on their intent, not proof of factual truth.".to_owned(),
        );
    }
    Ok(Confidence {
        level,
        reasons,
        independent_origins: i32::try_from(origins.len()).unwrap_or(i32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::records::{Provenance, RecordKind, Volatility, write};

    fn claim(review: Option<DateTimeOffset>) -> MemoryRecord {
        let content = ClaimContent {
            text: Some("c".to_owned()),
            category: MemoryCategory::Semantic,
            volatility: Volatility::Stable,
            last_verified: None,
            review_after: review,
        };
        MemoryRecord {
            id: Uuid::nil(),
            scope_id: Some("workspace".to_owned()),
            kind: RecordKind::Claim,
            status: KnowledgeStatus::Active,
            revision: 1,
            content_json: Some(write(&content).expect("written")),
            provenance: Some(Provenance {
                origin: ActorKind::User,
                actor: Some("john".to_owned()),
                timestamp: DateTimeOffset::now_utc(),
            }),
        }
    }

    fn reasons(claim: &MemoryRecord, now: DateTimeOffset) -> Vec<String> {
        evaluate_confidence(claim, &[], now, &|_| 0)
            .expect("evaluated")
            .reasons
    }

    #[test]
    fn the_review_date_passes_at_the_instant_itself() {
        let now = DateTimeOffset::now_utc();
        let passed = "Review date passed: revalidation required.".to_owned();
        assert_eq!(
            reasons(&claim(Some(now)), now),
            std::slice::from_ref(&passed)
        );
        let later = DateTimeOffset::new(now.utc_ticks() + 1, 0).expect("a date");
        assert!(!reasons(&claim(Some(later)), now).contains(&passed));
    }

    #[test]
    fn a_missing_content_or_provenance_is_refused() {
        let now = DateTimeOffset::now_utc();
        let mut no_content = claim(None);
        no_content.content_json = None;
        let error = evaluate_confidence(&no_content, &[], now, &|_| 0).expect_err("refused");
        assert_eq!(error.code, "invalid_storage_or_path");

        let mut evidence = claim(None);
        evidence.kind = RecordKind::Evidence;
        evidence.content_json = Some(
            write(&EvidenceContent {
                claim_id: Uuid::nil(),
                kind: EvidenceKind::SourceCode,
                reference: Some("r".to_owned()),
                lineage_id: Some("l".to_owned()),
                relation: EvidenceRelation::Supports,
                source_id: None,
                source_hash: None,
            })
            .expect("written"),
        );
        evidence.provenance = None;
        let error =
            evaluate_confidence(&claim(None), &[evidence], now, &|_| 0).expect_err("refused");
        assert_eq!(error.code, "invalid_storage_or_path");
    }
}
