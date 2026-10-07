using Sermofur.Domain;

namespace Sermofur.Application;

public sealed class MemoryService
{
    private readonly IMemoryStore store;
    private readonly MemoryContext context;

    public MemoryService(IMemoryStore store, MemoryContext context)
    {
        this.store = store;
        IReadOnlyList<Scope> scopes = store.ReadScopes();
        ScopePolicy.ValidateTree(scopes);
        this.context = context with
        {
            VisibleScopes = ScopePolicy.VisibleAncestors(context.ScopeId, scopes),
        };
    }

    public IReadOnlyList<MemoryRecord> List(RecordKind? kind = null) =>
        store.ReadRecords(context.VisibleScopes, kind);

    public MemoryRecord Get(Guid id) =>
        store.FindRecord(id, context.VisibleScopes)
        ?? throw new SermofurException("not_found", "Object missing or inaccessible.");

    public MemoryRecord CreateClaim(ClaimContent content, Provenance provenance, string? key)
    {
        ValidateText(content.Text);
        if (!Enum.IsDefined(content.Category) || !Enum.IsDefined(content.Volatility))
        {
            throw new SermofurException("invalid_input", "Invalid category or volatility.");
        }

        if (
            (content.Category is MemoryCategory.Preferences or MemoryCategory.Decisions)
            && provenance.Origin != ActorKind.User
        )
        {
            throw new SermofurException(
                "user_choice_required",
                "A personal choice requires a user origin."
            );
        }

        return Create(RecordKind.Claim, content, provenance, key);
    }

    public MemoryRecord CreateEvidence(EvidenceContent content, Provenance provenance, string? key)
    {
        if (!Enum.IsDefined(content.Kind))
        {
            throw new SermofurException("invalid_input", "Invalid evidence kind.");
        }

        ValidateText(content.Reference);
        ValidateText(content.LineageId);
        MemoryRecord claim = Get(content.ClaimId);
        if (claim.Kind != RecordKind.Claim || claim.ScopeId != context.ScopeId)
        {
            throw new SermofurException(
                "scope_boundary",
                "Evidence and claim must share the current scope.",
                4
            );
        }

        return Create(RecordKind.Evidence, content, provenance, key);
    }

    public MemoryRecord CreateRetex(RetexContent content, Provenance provenance, string? key)
    {
        ValidateText(content.Event);
        ValidateText(content.Impact);
        ValidateText(content.NextAction);
        return Create(RecordKind.Retex, content, provenance, key);
    }

    private MemoryRecord Create<T>(RecordKind kind, T content, Provenance provenance, string? key)
    {
        ValidateText(provenance.Actor);
        if (!Enum.IsDefined(provenance.Origin))
        {
            throw new SermofurException("invalid_input", "Invalid origin.");
        }

        if (key is not null)
        {
            ValidateText(key);
        }

        KnowledgeStatus status =
            kind == RecordKind.Retex ? KnowledgeStatus.Draft : KnowledgeStatus.Proposed;
        return store.CreateRecord(
            new MemoryRecord(
                Guid.NewGuid(),
                context.ScopeId,
                kind,
                status,
                1,
                RecordJson.Write(content),
                provenance
            ),
            key
        );
    }

    public ClaimExplanation Explain(Guid id)
    {
        MemoryRecord claim = Get(id);
        if (claim.Kind != RecordKind.Claim)
        {
            throw new SermofurException("wrong_kind", "A claim is required.");
        }

        MemoryRecord[] evidence = List(RecordKind.Evidence)
            .Where(record => RecordJson.Read<EvidenceContent>(record.ContentJson).ClaimId == id)
            .ToArray();
        return new ClaimExplanation(
            claim,
            EvaluateConfidence(claim, evidence),
            evidence,
            store.ReadHistory(id, context.VisibleScopes)
        );
    }

    public MemoryRecord Invalidate(Guid id, string reason, string actor)
    {
        ValidateText(reason);
        ValidateText(actor);
        MemoryRecord record = Get(id);
        if (record.Kind != RecordKind.Claim || record.ScopeId != context.ScopeId)
        {
            throw new SermofurException(
                "scope_boundary",
                "Only a claim of the current scope can be invalidated.",
                4
            );
        }

        return store.InvalidateRecord(id, context.ScopeId, reason, actor);
    }

    public IReadOnlyList<string> Export() => store.Export(context.VisibleScopes);

    public static Confidence EvaluateConfidence(
        MemoryRecord claim,
        IReadOnlyList<MemoryRecord> evidence
    )
    {
        if (claim.Status is KnowledgeStatus.Invalidated or KnowledgeStatus.Superseded)
        {
            return new Confidence(
                ConfidenceLevel.Low,
                ["Knowledge not applicable: invalidated or superseded."],
                0
            );
        }

        ClaimContent content = RecordJson.Read<ClaimContent>(claim.ContentJson);
        if (content.ReviewAfter <= DateTimeOffset.UtcNow)
        {
            return new Confidence(
                ConfidenceLevel.Low,
                ["Review date passed: revalidation required."],
                0
            );
        }

        MemoryRecord[] trustedEvidence = evidence
            .Where(e => e.Provenance.Origin != ActorKind.Llm)
            .ToArray();
        IGrouping<string, EvidenceContent>[] origins = trustedEvidence
            .Select(e => RecordJson.Read<EvidenceContent>(e.ContentJson))
            .GroupBy(e => e.LineageId, StringComparer.Ordinal)
            .ToArray();
        List<string> reasons = new List<string>();
        ConfidenceLevel level = ConfidenceLevel.Low;
        foreach (IGrouping<string, EvidenceContent> origin in origins)
        {
            ConfidenceLevel support = origin.Max(e =>
                e.Kind switch
                {
                    EvidenceKind.SourceCode or EvidenceKind.AuthoritativeDocumentation =>
                        ConfidenceLevel.High,
                    EvidenceKind.ProjectDecision
                    or EvidenceKind.LocalDocumentation
                    or EvidenceKind.HumanObservation => ConfidenceLevel.Medium,
                    _ => ConfidenceLevel.Low,
                }
            );
            level = (ConfidenceLevel)Math.Max((int)level, (int)support);
            reasons.Add($"Origin {origin.Key}: support {support}; repetitions grouped.");
        }
        if (evidence.Count != trustedEvidence.Length)
        {
            reasons.Add(
                "LLM assertions excluded from reinforcement, whatever the declared source kind."
            );
        }

        if (reasons.Count == 0)
        {
            reasons.Add("No independent evidence available.");
        }

        reasons.Add("Declared evidence; no controlled execution collector in this version.");
        if (content.Category is MemoryCategory.Preferences or MemoryCategory.Decisions)
        {
            reasons.Add("User choice authoritative on their intent, not proof of factual truth.");
        }

        return new Confidence(level, reasons.ToArray(), origins.Length);
    }

    public static void ValidateText(string? value)
    {
        if (string.IsNullOrWhiteSpace(value) || InputLimits.IsOutOfBounds(value))
        {
            throw new SermofurException("invalid_input", InputLimits.TextMessage);
        }
    }
}
