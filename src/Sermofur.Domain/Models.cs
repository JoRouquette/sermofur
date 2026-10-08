namespace Sermofur.Domain;

public enum ScopeKind
{
    Workspace,
    Client,
    Project,
    Repository,
    Task,
}

public enum RecordKind
{
    Claim,
    Evidence,
    Retex,
    Source,
}

public enum MemoryCategory
{
    Episodic,
    Semantic,
    Procedural,
    Preferences,
    Decisions,
}

public enum KnowledgeStatus
{
    Proposed,
    Active,
    Invalidated,
    Superseded,
    Draft,
}

public enum ActorKind
{
    User,
    Llm,
}

public enum Volatility
{
    Stable,
    Evolving,
    Volatile,
}

public enum EvidenceKind
{
    Execution,
    SourceCode,
    AuthoritativeDocumentation,
    ProjectDecision,
    LocalDocumentation,
    HumanObservation,
    UserAssertion,
    LlmAssertion,
}

public enum ConfidenceLevel
{
    Low,
    Medium,
    High,
    Verified,
}

public sealed record Scope(string Id, ScopeKind Kind, string? ParentId, string? RelativePath);

public sealed record InstanceConfiguration(
    int SchemaVersion,
    Guid InstanceId,
    DateTimeOffset CreatedAt
);

public sealed record Provenance(ActorKind Origin, string Actor, DateTimeOffset Timestamp);

public sealed record ClaimContent(
    string Text,
    MemoryCategory Category,
    Volatility Volatility,
    DateTimeOffset? LastVerified = null,
    DateTimeOffset? ReviewAfter = null
);

/// <summary>Whether a piece of evidence supports or contradicts its claim.</summary>
public enum EvidenceRelation
{
    Supports,
    Contradicts,
}

/// <summary>
/// Evidence of a claim. <see cref="Relation"/>, <see cref="SourceId"/> and
/// <see cref="SourceHash"/> came with format 2; a format 1 payload reads as supporting evidence
/// that cites no source.
/// </summary>
public sealed record EvidenceContent(
    Guid ClaimId,
    EvidenceKind Kind,
    string Reference,
    string LineageId,
    EvidenceRelation Relation = EvidenceRelation.Supports,
    Guid? SourceId = null,
    string? SourceHash = null
);

/// <summary>State of a declared source after its last indexing attempt.</summary>
public enum SourceStatus
{
    Indexed,
    Missing,
    Unreadable,
    Rejected,
}

/// <summary>Why a source file cannot be indexed.</summary>
public enum SourceRejection
{
    TooLarge,
    Binary,
    Empty,
    UnsafePath,
}

/// <summary>
/// A local text file declared as a source. <see cref="Hash"/>, <see cref="Size"/> and
/// <see cref="IndexedAt"/> describe the bytes last indexed, kept when the file later goes
/// missing; the indexed text itself lives only in the search index.
/// </summary>
public sealed record SourceContent(
    string RelativePath,
    SourceStatus Status,
    string Hash,
    long Size,
    DateTimeOffset? IndexedAt,
    int Passages,
    SourceRejection? Rejection = null
);

public sealed record RetexContent(string Event, string Impact, string NextAction);

public sealed record MemoryRecord(
    Guid Id,
    string ScopeId,
    RecordKind Kind,
    KnowledgeStatus Status,
    int Revision,
    string ContentJson,
    Provenance Provenance
);

public sealed record HistoryEntry(
    long Sequence,
    Guid ObjectId,
    string ScopeId,
    string? PreviousState,
    string NewState,
    string Reason,
    string Actor,
    DateTimeOffset Timestamp
);

public sealed record Confidence(ConfidenceLevel Level, string[] Reasons, int IndependentOrigins);

public sealed record ClaimExplanation(
    MemoryRecord Claim,
    Confidence Confidence,
    IReadOnlyList<MemoryRecord> Evidence,
    IReadOnlyList<HistoryEntry> History
);

public sealed class SermofurException(string code, string message, int exitCode = 1)
    : Exception(message)
{
    public string Code { get; } = code;
    public int ExitCode { get; } = exitCode;
}
