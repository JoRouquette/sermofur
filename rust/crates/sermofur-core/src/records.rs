//! The records of the domain (`Sermofur.Domain/Models.cs`) and their JSON forms, as `RecordJson`
//! writes and reads them (`rust/tests/golden/records.json` is the truth): properties in
//! camelCase, enumerations in snake_case.
//!
//! `RecordJson.Read` is lenient where `System.Text.Json` is: a missing property takes its
//! default (`null` for a string, the first value of an enumeration, the empty GUID, 0), so a
//! string read back may be `null`, hence the `Option<String>` fields.

use serde::{Serialize, Serializer};
use uuid::Uuid;

use crate::error::Result;
use crate::format::DateTimeOffset;
use crate::instance::invalid_storage;
use crate::stj::{self, Node};

/// An enumeration of the domain: written in snake_case; read as `JsonStringEnumConverter`
/// with `allowIntegerValues: false` reads it (see [`parse_enum`]).
pub trait DotNetEnum: Sized + Copy + 'static {
    /// Name in the .NET source, name in JSON, and value, in declaration order (the value of
    /// each member is its index).
    const NAMES: &'static [(&'static str, &'static str, Self)];
}

macro_rules! dotnet_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $json:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl DotNetEnum for $name {
            const NAMES: &'static [(&'static str, &'static str, Self)] =
                &[$((stringify!($variant), $json, $name::$variant)),+];
        }

        /// `default(T)`: the first member, which a missing property takes.
        impl Default for $name {
            fn default() -> Self {
                Self::NAMES[0].2
            }
        }
    };
}

dotnet_enum!(ScopeKind {
    Workspace = "workspace",
    Client = "client",
    Project = "project",
    Repository = "repository",
    Task = "task",
});
dotnet_enum!(RecordKind {
    Claim = "claim",
    Evidence = "evidence",
    Retex = "retex",
    Source = "source",
});
dotnet_enum!(MemoryCategory {
    Episodic = "episodic",
    Semantic = "semantic",
    Procedural = "procedural",
    Preferences = "preferences",
    Decisions = "decisions",
});
dotnet_enum!(KnowledgeStatus {
    Proposed = "proposed",
    Active = "active",
    Invalidated = "invalidated",
    Superseded = "superseded",
    Draft = "draft",
});
dotnet_enum!(ActorKind {
    User = "user",
    Llm = "llm",
});
dotnet_enum!(Volatility {
    Stable = "stable",
    Evolving = "evolving",
    Volatile = "volatile",
});
dotnet_enum!(EvidenceKind {
    Execution = "execution",
    SourceCode = "source_code",
    AuthoritativeDocumentation = "authoritative_documentation",
    ProjectDecision = "project_decision",
    LocalDocumentation = "local_documentation",
    HumanObservation = "human_observation",
    UserAssertion = "user_assertion",
    LlmAssertion = "llm_assertion",
});
dotnet_enum!(ConfidenceLevel {
    Low = "low",
    Medium = "medium",
    High = "high",
    Verified = "verified",
});
dotnet_enum!(
    /// Whether a piece of evidence supports or contradicts its claim.
    EvidenceRelation {
        Supports = "supports",
        Contradicts = "contradicts",
    }
);
dotnet_enum!(
    /// State of a declared source after its last indexing attempt.
    SourceStatus {
        Indexed = "indexed",
        Missing = "missing",
        Unreadable = "unreadable",
        Rejected = "rejected",
    }
);
dotnet_enum!(
    /// Why a source file cannot be indexed.
    SourceRejection {
        TooLarge = "too_large",
        Binary = "binary",
        Empty = "empty",
        UnsafePath = "unsafe_path",
    }
);

/// A member read as `JsonStringEnumConverter` reads it: the JSON or the .NET name without
/// case, spaces around ignored; a list separated by commas combines its members by OR, as
/// `Enum.TryParse`; a number, even as text, is refused. A combination that is not a member is
/// refused too: the oracle fails on it (`records.json`, "evolving, volatile").
fn parse_enum<E: DotNetEnum>(node: &Node) -> Result<E> {
    let raw = stj::required_text(node)?;
    let trimmed = raw.trim();
    if trimmed
        .chars()
        .next()
        .is_none_or(|c| c.is_ascii_digit() || c == '-' || c == '+')
    {
        return Err(invalid_storage());
    }
    let mut value = 0usize;
    for part in trimmed.split(',') {
        let part = part.trim();
        let index = E::NAMES
            .iter()
            .position(|(dotnet, json, _)| {
                dotnet.eq_ignore_ascii_case(part) || json.eq_ignore_ascii_case(part)
            })
            .ok_or_else(invalid_storage)?;
        value |= index;
    }
    E::NAMES
        .get(value)
        .map(|(_, _, member)| *member)
        .ok_or_else(invalid_storage)
}

fn optional_enum<E: DotNetEnum>(node: &Node) -> Result<Option<E>> {
    if *node == Node::Null {
        Ok(None)
    } else {
        parse_enum(node).map(Some)
    }
}

pub(crate) fn guid_text<S: Serializer>(
    value: &Uuid,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.hyphenated().to_string())
}

fn optional_guid_text<S: Serializer>(
    value: &Option<Uuid>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value {
        Some(value) => guid_text(value, serializer),
        None => serializer.serialize_none(),
    }
}

/// `default(DateTimeOffset)`: `0001-01-01T00:00:00+00:00`.
fn default_date() -> DateTimeOffset {
    DateTimeOffset::new(0, 0).unwrap_or_else(DateTimeOffset::now_utc)
}

/// A record that `RecordJson.Read` reads.
pub trait Record: Sized {
    /// The record from the properties of its object, in document order.
    fn from_properties(
        properties: &[(&str, &Node)],
        local_offset_minutes: &dyn Fn(i64) -> i16,
    ) -> Result<Self>;
}

/// `RecordJson.Read<T>`: a JSON error is `invalid_storage_or_path`, a top-level `null`
/// `invalid_record`. `local_offset_minutes` serves a date without offset.
pub fn read<T: Record>(json: &str, local_offset_minutes: &dyn Fn(i64) -> i16) -> Result<T> {
    let node = stj::parse(json)?;
    T::from_properties(&stj::document(&node)?, local_offset_minutes)
}

/// `RecordJson.Write`: indented JSON with the escaping and line breaks of .NET.
pub fn write<T: Serialize>(value: &T) -> Result<String> {
    crate::json::to_string(value).map_err(|_| invalid_storage())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub id: String,
    pub kind: ScopeKind,
    pub parent_id: Option<String>,
    pub relative_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub origin: ActorKind,
    pub actor: Option<String>,
    pub timestamp: DateTimeOffset,
}

impl Record for Provenance {
    fn from_properties(properties: &[(&str, &Node)], offset: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = Provenance {
            origin: ActorKind::default(),
            actor: None,
            timestamp: default_date(),
        };
        for (name, value) in properties {
            match *name {
                "origin" => out.origin = parse_enum(value)?,
                "actor" => out.actor = stj::text(value)?,
                "timestamp" => out.timestamp = stj::date(value, offset)?,
                _ => {}
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimContent {
    pub text: Option<String>,
    pub category: MemoryCategory,
    pub volatility: Volatility,
    pub last_verified: Option<DateTimeOffset>,
    pub review_after: Option<DateTimeOffset>,
}

impl Record for ClaimContent {
    fn from_properties(properties: &[(&str, &Node)], offset: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = ClaimContent {
            text: None,
            category: MemoryCategory::default(),
            volatility: Volatility::default(),
            last_verified: None,
            review_after: None,
        };
        for (name, value) in properties {
            match *name {
                "text" => out.text = stj::text(value)?,
                "category" => out.category = parse_enum(value)?,
                "volatility" => out.volatility = parse_enum(value)?,
                "lastVerified" => out.last_verified = stj::optional_date(value, offset)?,
                "reviewAfter" => out.review_after = stj::optional_date(value, offset)?,
                _ => {}
            }
        }
        Ok(out)
    }
}

/// Evidence of a claim; `relation`, `source_id` and `source_hash` came with format 2, so a
/// format 1 payload reads as supporting evidence that cites no source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceContent {
    #[serde(serialize_with = "guid_text")]
    pub claim_id: Uuid,
    pub kind: EvidenceKind,
    pub reference: Option<String>,
    pub lineage_id: Option<String>,
    pub relation: EvidenceRelation,
    #[serde(serialize_with = "optional_guid_text")]
    pub source_id: Option<Uuid>,
    pub source_hash: Option<String>,
}

impl Record for EvidenceContent {
    fn from_properties(properties: &[(&str, &Node)], _: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = EvidenceContent {
            claim_id: Uuid::nil(),
            kind: EvidenceKind::default(),
            reference: None,
            lineage_id: None,
            relation: EvidenceRelation::Supports,
            source_id: None,
            source_hash: None,
        };
        for (name, value) in properties {
            match *name {
                "claimId" => out.claim_id = stj::guid(value)?,
                "kind" => out.kind = parse_enum(value)?,
                "reference" => out.reference = stj::text(value)?,
                "lineageId" => out.lineage_id = stj::text(value)?,
                "relation" => out.relation = parse_enum(value)?,
                "sourceId" => out.source_id = stj::optional_guid(value)?,
                "sourceHash" => out.source_hash = stj::text(value)?,
                _ => {}
            }
        }
        Ok(out)
    }
}

/// A local text file declared as a source; `hash`, `size` and `indexed_at` describe the bytes
/// last indexed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceContent {
    pub relative_path: Option<String>,
    pub status: SourceStatus,
    pub hash: Option<String>,
    pub size: i64,
    pub indexed_at: Option<DateTimeOffset>,
    pub passages: i32,
    pub rejection: Option<SourceRejection>,
}

impl Record for SourceContent {
    fn from_properties(properties: &[(&str, &Node)], offset: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = SourceContent {
            relative_path: None,
            status: SourceStatus::default(),
            hash: None,
            size: 0,
            indexed_at: None,
            passages: 0,
            rejection: None,
        };
        for (name, value) in properties {
            match *name {
                "relativePath" => out.relative_path = stj::text(value)?,
                "status" => out.status = parse_enum(value)?,
                "hash" => out.hash = stj::text(value)?,
                "size" => out.size = stj::integer(value)?,
                "indexedAt" => out.indexed_at = stj::optional_date(value, offset)?,
                "passages" => out.passages = stj::integer(value)?,
                "rejection" => out.rejection = optional_enum(value)?,
                _ => {}
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetexContent {
    pub event: Option<String>,
    pub impact: Option<String>,
    pub next_action: Option<String>,
}

impl Record for RetexContent {
    fn from_properties(properties: &[(&str, &Node)], _: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = RetexContent {
            event: None,
            impact: None,
            next_action: None,
        };
        for (name, value) in properties {
            match *name {
                "event" => out.event = stj::text(value)?,
                "impact" => out.impact = stj::text(value)?,
                "nextAction" => out.next_action = stj::text(value)?,
                _ => {}
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    #[serde(serialize_with = "guid_text")]
    pub id: Uuid,
    pub scope_id: Option<String>,
    pub kind: RecordKind,
    pub status: KnowledgeStatus,
    pub revision: i32,
    pub content_json: Option<String>,
    pub provenance: Option<Provenance>,
}

impl Record for MemoryRecord {
    fn from_properties(properties: &[(&str, &Node)], offset: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = MemoryRecord {
            id: Uuid::nil(),
            scope_id: None,
            kind: RecordKind::default(),
            status: KnowledgeStatus::default(),
            revision: 0,
            content_json: None,
            provenance: None,
        };
        for (name, value) in properties {
            match *name {
                "id" => out.id = stj::guid(value)?,
                "scopeId" => out.scope_id = stj::text(value)?,
                "kind" => out.kind = parse_enum(value)?,
                "status" => out.status = parse_enum(value)?,
                "revision" => out.revision = stj::integer(value)?,
                "contentJson" => out.content_json = stj::text(value)?,
                "provenance" => {
                    out.provenance = match stj::object(value)? {
                        Some(inner) => Some(Provenance::from_properties(&inner, offset)?),
                        None => None,
                    };
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub sequence: i64,
    #[serde(serialize_with = "guid_text")]
    pub object_id: Uuid,
    pub scope_id: Option<String>,
    pub previous_state: Option<String>,
    pub new_state: Option<String>,
    pub reason: Option<String>,
    pub actor: Option<String>,
    pub timestamp: DateTimeOffset,
}

impl Record for HistoryEntry {
    fn from_properties(properties: &[(&str, &Node)], offset: &dyn Fn(i64) -> i16) -> Result<Self> {
        let mut out = HistoryEntry {
            sequence: 0,
            object_id: Uuid::nil(),
            scope_id: None,
            previous_state: None,
            new_state: None,
            reason: None,
            actor: None,
            timestamp: default_date(),
        };
        for (name, value) in properties {
            match *name {
                "sequence" => out.sequence = stj::integer(value)?,
                "objectId" => out.object_id = stj::guid(value)?,
                "scopeId" => out.scope_id = stj::text(value)?,
                "previousState" => out.previous_state = stj::text(value)?,
                "newState" => out.new_state = stj::text(value)?,
                "reason" => out.reason = stj::text(value)?,
                "actor" => out.actor = stj::text(value)?,
                "timestamp" => out.timestamp = stj::date(value, offset)?,
                _ => {}
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Confidence {
    pub level: ConfidenceLevel,
    pub reasons: Vec<String>,
    pub independent_origins: i32,
}
