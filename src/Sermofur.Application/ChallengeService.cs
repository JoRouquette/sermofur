using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>One reason to doubt a claim, with what supports the doubt.</summary>
public sealed record ChallengeSignal(
    string Type,
    Guid ObjectId,
    string Explanation,
    Guid? EvidenceId = null,
    Guid? SourceId = null,
    string? RecordedHash = null,
    string? CurrentHash = null
);

public sealed record ChallengeTarget(Guid? ClaimId, string Text, string? Status);

public sealed record ChallengeAnswer(
    ChallengeTarget Target,
    IReadOnlyList<ChallengeSignal> Signals,
    IReadOnlyList<RecallResult> ToConfront
);

/// <summary>
/// Confronts a claim, or a text about to become one, with what is declared or measurable:
/// contradicting evidence, cited sources that changed, status and review date (FR-017), plus
/// close claims to confront, never called contradictions (FR-018). Writes nothing.
/// </summary>
public sealed class ChallengeService(MemoryService memory, RecallService recall)
{
    public ChallengeAnswer Challenge(Guid claimId)
    {
        MemoryRecord claim = memory.Get(claimId);
        if (claim.Kind != RecordKind.Claim)
        {
            throw new SermofurException("not_found", "Object missing or inaccessible.");
        }
        ClaimContent content = RecordJson.Read<ClaimContent>(claim.ContentJson);
        List<ChallengeSignal> signals = new List<ChallengeSignal>();
        if (claim.Status is KnowledgeStatus.Invalidated or KnowledgeStatus.Superseded)
        {
            IReadOnlyList<HistoryEntry> history = memory.Explain(claimId).History;
            string reason = history.Count > 0 ? history[^1].Reason : "no reason recorded";
            signals.Add(
                new(
                    "not_applicable",
                    claim.Id,
                    $"Claim {claim.Status.ToString().ToLowerInvariant()}: {reason}."
                )
            );
        }
        if (content.ReviewAfter <= DateTimeOffset.UtcNow)
        {
            signals.Add(
                new(
                    "review_due",
                    claim.Id,
                    $"Review date passed on {content.ReviewAfter:O}: revalidation required."
                )
            );
        }
        foreach (MemoryRecord evidence in memory.List(RecordKind.Evidence))
        {
            EvidenceContent proof = RecordJson.Read<EvidenceContent>(evidence.ContentJson);
            if (proof.ClaimId != claim.Id)
            {
                continue;
            }
            if (proof.Relation == EvidenceRelation.Contradicts)
            {
                signals.Add(Contradiction(claim, evidence, proof));
            }
            if (proof.SourceId is Guid sourceId)
            {
                ChallengeSignal? source = SourceSignal(claim, evidence, proof, sourceId);
                if (source is not null)
                {
                    signals.Add(source);
                }
            }
        }
        return new ChallengeAnswer(
            new ChallengeTarget(claim.Id, content.Text, claim.Status.ToString().ToLowerInvariant()),
            signals,
            recall.CloseClaims(content.Text, claim.Id)
        );
    }

    public ChallengeAnswer Challenge(string text)
    {
        MemoryService.ValidateText(text);
        return new ChallengeAnswer(
            new ChallengeTarget(null, text, null),
            [],
            recall.CloseClaims(text, null)
        );
    }

    private static ChallengeSignal Contradiction(
        MemoryRecord claim,
        MemoryRecord evidence,
        EvidenceContent proof
    )
    {
        string origin =
            evidence.Provenance.Origin == ActorKind.Llm
                ? " LLM assertion: noted, not an independent refutation."
                : string.Empty;
        return new ChallengeSignal(
            "contradiction",
            claim.Id,
            $"Contradicted by {Snake(proof.Kind)} evidence \"{proof.Reference}\" (origin {proof.LineageId}, "
                + $"{evidence.Provenance.Origin.ToString().ToLowerInvariant()} {evidence.Provenance.Actor}, "
                + $"{evidence.Provenance.Timestamp:O}).{origin}",
            evidence.Id
        );
    }

    private ChallengeSignal? SourceSignal(
        MemoryRecord claim,
        MemoryRecord evidence,
        EvidenceContent proof,
        Guid sourceId
    )
    {
        MemoryRecord source;
        try
        {
            source = memory.Get(sourceId);
        }
        catch (SermofurException exception) when (exception.Code == "not_found")
        {
            return null;
        }
        SourceContent cited = RecordJson.Read<SourceContent>(source.ContentJson);
        if (cited.Status != SourceStatus.Indexed)
        {
            return new ChallengeSignal(
                "source_unavailable",
                claim.Id,
                $"Source {cited.RelativePath} cited by evidence \"{proof.Reference}\" is "
                    + $"{cited.Status.ToString().ToLowerInvariant()} since its last reindexing.",
                evidence.Id,
                source.Id,
                proof.SourceHash,
                null
            );
        }
        if (cited.Hash == proof.SourceHash)
        {
            return null;
        }
        return new ChallengeSignal(
            "source_changed",
            claim.Id,
            $"Source {cited.RelativePath} changed since evidence \"{proof.Reference}\" was recorded.",
            evidence.Id,
            source.Id,
            proof.SourceHash,
            cited.Hash
        );
    }

    private static string Snake(EvidenceKind kind) =>
        string.Concat(
            kind.ToString()
                .Select(
                    (c, i) =>
                        i > 0 && char.IsUpper(c)
                            ? "_" + char.ToLowerInvariant(c)
                            : char.ToLowerInvariant(c).ToString()
                )
        );
}
