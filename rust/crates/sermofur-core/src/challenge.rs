//! Challenge, as `ChallengeService` of the .NET oracle (`rust/tests/golden/challenge.json` is the
//! truth): confronts a claim, or a text about to become one, with what is declared or measurable
//! (contradicting evidence, cited sources that changed or became unavailable, status, review
//! date), plus at most 3 close claims to confront, never called contradictions. Writes nothing.
//!
//! Signals come in a fixed order: `not_applicable`, `review_due`, then, for each visible evidence
//! of the claim in list order, its `contradiction` before its source signal.

use serde::Serialize;
use uuid::Uuid;

use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::instance::invalid_storage;
use crate::memory::{MemoryContext, validate_text};
use crate::recall::{RecallResult, close_claims, lower_name};
use crate::records::{
    ActorKind, ClaimContent, DotNetEnum, EvidenceContent, EvidenceRelation, KnowledgeStatus,
    MemoryRecord, Record, RecordKind, SourceContent, SourceStatus, guid_text, optional_guid_text,
    read,
};
use crate::store::Store;

/// One reason to doubt a claim, with what supports the doubt (`ChallengeSignal`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeSignal {
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(serialize_with = "guid_text")]
    pub object_id: Uuid,
    pub explanation: String,
    #[serde(serialize_with = "optional_guid_text")]
    pub evidence_id: Option<Uuid>,
    #[serde(serialize_with = "optional_guid_text")]
    pub source_id: Option<Uuid>,
    pub recorded_hash: Option<String>,
    pub current_hash: Option<String>,
}

/// What is challenged (`ChallengeTarget`): a claim, or a text without identifier nor status.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeTarget {
    #[serde(serialize_with = "optional_guid_text")]
    pub claim_id: Option<Uuid>,
    pub text: String,
    pub status: Option<String>,
}

/// The answer of `challenge` (`ChallengeAnswer`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeAnswer {
    pub target: ChallengeTarget,
    pub signals: Vec<ChallengeSignal>,
    pub to_confront: Vec<RecallResult>,
}

fn content<T: Record>(record: &MemoryRecord) -> Result<T> {
    read(
        record.content_json.as_deref().ok_or_else(invalid_storage)?,
        &|_| 0,
    )
}

fn signal(kind: &'static str, claim: Uuid, explanation: String) -> ChallengeSignal {
    ChallengeSignal {
        kind,
        object_id: claim,
        explanation,
        evidence_id: None,
        source_id: None,
        recorded_hash: None,
        current_hash: None,
    }
}

/// `ChallengeService.Challenge(Guid)`: the signals about a visible claim. `now` decides whether
/// the review date has passed.
pub fn challenge_claim(
    store: &Store,
    context: &MemoryContext,
    id: Uuid,
    now: DateTimeOffset,
) -> Result<ChallengeAnswer> {
    let context = context.checked(store)?;
    let visible = &context.visible;
    let claim = store
        .find_record(id, visible)?
        .filter(|record| record.kind == RecordKind::Claim)
        .ok_or_else(|| SermofurError::new("not_found", "Object missing or inaccessible.", 1))?;
    let claim_content: ClaimContent = content(&claim)?;
    let mut signals = Vec::new();
    if matches!(
        claim.status,
        KnowledgeStatus::Invalidated | KnowledgeStatus::Superseded
    ) {
        let history = store.read_history(id, visible)?;
        let reason = history.last().map_or("no reason recorded", |entry| {
            entry.reason.as_deref().unwrap_or_default()
        });
        signals.push(signal(
            "not_applicable",
            claim.id,
            format!("Claim {}: {reason}.", lower_name(claim.status)),
        ));
    }
    if let Some(review) = claim_content.review_after
        && review.utc_ticks() <= now.utc_ticks()
    {
        signals.push(signal(
            "review_due",
            claim.id,
            format!(
                "Review date passed on {}: revalidation required.",
                review.to_o()
            ),
        ));
    }
    for evidence in store.read_records(visible, Some(RecordKind::Evidence))? {
        let proof: EvidenceContent = content(&evidence)?;
        if proof.claim_id != claim.id {
            continue;
        }
        if proof.relation == EvidenceRelation::Contradicts {
            signals.push(contradiction(&claim, &evidence, &proof)?);
        }
        if let Some(source_id) = proof.source_id
            && let Some(source) =
                source_signal(store, visible, &claim, &evidence, &proof, source_id)?
        {
            signals.push(source);
        }
    }
    // A claim without text cannot be stored; .NET fails on it in the tokenizer
    // (`InvalidOperationException`, so `invalid_storage_or_path`).
    let text = claim_content.text.ok_or_else(invalid_storage)?;
    let to_confront = close_claims(store, &context, &text, Some(claim.id), now)?;
    Ok(ChallengeAnswer {
        target: ChallengeTarget {
            claim_id: Some(claim.id),
            text,
            status: Some(lower_name(claim.status)),
        },
        signals,
        to_confront,
    })
}

/// `ChallengeService.Challenge(string)`: the close claims of a text that is not a claim yet.
pub fn challenge_text(
    store: &Store,
    context: &MemoryContext,
    text: &str,
    now: DateTimeOffset,
) -> Result<ChallengeAnswer> {
    let context = context.checked(store)?;
    validate_text(text)?;
    Ok(ChallengeAnswer {
        target: ChallengeTarget {
            claim_id: None,
            text: text.to_owned(),
            status: None,
        },
        signals: Vec::new(),
        to_confront: close_claims(store, &context, text, None, now)?,
    })
}

/// The JSON name of a member (`JsonNamingPolicy.SnakeCaseLower`).
fn snake<E: DotNetEnum + PartialEq>(member: E) -> &'static str {
    E::NAMES
        .iter()
        .find(|(_, _, m)| *m == member)
        .map_or("", |(_, json, _)| json)
}

fn contradiction(
    claim: &MemoryRecord,
    evidence: &MemoryRecord,
    proof: &EvidenceContent,
) -> Result<ChallengeSignal> {
    // A missing provenance is a `NullReferenceException` in .NET (corrupt store only).
    let provenance = evidence.provenance.as_ref().ok_or_else(invalid_storage)?;
    let origin = if provenance.origin == ActorKind::Llm {
        " LLM assertion: noted, not an independent refutation."
    } else {
        ""
    };
    Ok(ChallengeSignal {
        evidence_id: Some(evidence.id),
        ..signal(
            "contradiction",
            claim.id,
            format!(
                "Contradicted by {} evidence \"{}\" (origin {}, {} {}, {}).{origin}",
                snake(proof.kind),
                proof.reference.as_deref().unwrap_or_default(),
                proof.lineage_id.as_deref().unwrap_or_default(),
                lower_name(provenance.origin),
                provenance.actor.as_deref().unwrap_or_default(),
                provenance.timestamp.to_o(),
            ),
        )
    })
}

/// The signal of a cited source: unavailable when out of reach or not indexed, changed when its
/// hash differs from the one frozen with the evidence (ordinal text, `null` included).
fn source_signal(
    store: &Store,
    visible: &std::collections::HashSet<String>,
    claim: &MemoryRecord,
    evidence: &MemoryRecord,
    proof: &EvidenceContent,
    source_id: Uuid,
) -> Result<Option<ChallengeSignal>> {
    let reference = proof.reference.as_deref().unwrap_or_default();
    let cited_signal = |kind, explanation, source, current| ChallengeSignal {
        evidence_id: Some(evidence.id),
        source_id: Some(source),
        recorded_hash: proof.source_hash.clone(),
        current_hash: current,
        ..signal(kind, claim.id, explanation)
    };
    let Some(source) = store.find_record(source_id, visible)? else {
        // Moved to a narrower scope or otherwise out of reach: the path is not revealed.
        return Ok(Some(cited_signal(
            "source_unavailable",
            format!(
                "Source cited by evidence \"{reference}\" is no longer visible from this scope."
            ),
            source_id,
            None,
        )));
    };
    let cited: SourceContent = content(&source)?;
    let path = cited.relative_path.as_deref().unwrap_or_default();
    if cited.status != SourceStatus::Indexed {
        return Ok(Some(cited_signal(
            "source_unavailable",
            format!(
                "Source {path} cited by evidence \"{reference}\" is {} since its last reindexing.",
                lower_name(cited.status)
            ),
            source.id,
            None,
        )));
    }
    if cited.hash == proof.source_hash {
        return Ok(None);
    }
    Ok(Some(cited_signal(
        "source_changed",
        format!("Source {path} changed since evidence \"{reference}\" was recorded."),
        source.id,
        cited.hash,
    )))
}
