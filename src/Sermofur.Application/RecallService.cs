using System.Globalization;
using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>Frequency of a query term in an indexed passage, visible scopes only.</summary>
public sealed record TermHit(
    long Document,
    Guid ObjectId,
    string ScopeId,
    string Kind,
    int Passage,
    int Length,
    string Term,
    int Frequency
);

/// <summary>Size of the visible part of the index: what BM25 statistics are computed on.</summary>
public sealed record VisibleCorpus(int Documents, double AverageLength);

/// <summary>Read side of the full-text index. Every query is restricted to visible scopes.</summary>
public interface ISearchIndex
{
    ITokenizer Tokenizer { get; }

    IReadOnlyList<TermHit> Hits(IReadOnlyList<string> terms, IReadOnlySet<string> visible);

    VisibleCorpus Corpus(IReadOnlySet<string> visible);

    string PassageText(long document);
}

/// <summary>Result of an index rebuild, counted on visible objects only.</summary>
public sealed record IndexRebuild(int Indexed, int ChangedSources);

public sealed record Freshness(
    DateTimeOffset CreatedAt,
    DateTimeOffset? LastVerified = null,
    DateTimeOffset? ReviewAfter = null,
    DateTimeOffset? IndexedAt = null,
    string? Hash = null,
    string? SourceStatus = null
);

public sealed record RecallResult(
    string Kind,
    Guid Id,
    string ScopeId,
    string Status,
    bool Applicable,
    Confidence? Confidence,
    string Excerpt,
    IReadOnlyList<string> MatchedTerms,
    Freshness Freshness,
    double Score,
    string? Path = null
);

public sealed record RecallAnswer(string Query, IReadOnlyList<RecallResult> Results, int Excluded);

/// <summary>
/// Recall under a budget of 3 results (FR-010). Visibility filters candidates before any
/// scoring, and BM25 statistics (document count, document frequency, average length) are
/// computed on visible passages only, so that nothing invisible changes presence, order or
/// explanation of a result (FR-011, ADR 0013).
/// </summary>
public sealed class RecallService
{
    public const int MaxResults = 3;
    private const double K1 = 1.2;
    private const double B = 0.75;
    private const int ExcerptLength = 300;

    private readonly IMemoryStore store;
    private readonly ISearchIndex index;
    private readonly MemoryContext context;

    public RecallService(IMemoryStore store, ISearchIndex index, MemoryContext context)
    {
        this.store = store;
        this.index = index;
        IReadOnlyList<Scope> scopes = store.ReadScopes();
        ScopePolicy.ValidateTree(scopes);
        this.context = context with
        {
            VisibleScopes = ScopePolicy.VisibleAncestors(context.ScopeId, scopes),
        };
    }

    public RecallAnswer Recall(string query, int limit = MaxResults)
    {
        MemoryService.ValidateText(query);
        if (limit is < 1 or > MaxResults)
        {
            throw new SermofurException("invalid_arguments", "The limit is between 1 and 3.");
        }
        Selection selection = Select(query, kinds: null, exclude: null, limit, fill: true);
        return new RecallAnswer(query, selection.Results, selection.Excluded);
    }

    /// <summary>Applicable visible claims close to a text, for challenge; at most 3.</summary>
    public IReadOnlyList<RecallResult> CloseClaims(string text, Guid? exclude) =>
        Select(text, kinds: ["claim"], exclude, MaxResults, fill: false).Results;

    /// <summary>Highest confidence weight: bounds how much a description can raise a score.</summary>
    private const double MaxWeight = 1.2;

    private sealed record Candidate(
        long Document,
        RecordSummary Record,
        double Relevance,
        IReadOnlyList<string> Terms,
        bool Applicable
    );

    private sealed record Selection(IReadOnlyList<RecallResult> Results, int Excluded);

    /// <summary>
    /// Scores every visible match by BM25 on visible statistics, then describes only what is
    /// shown: confidence weights (at most x1.2) are computed in order of relevance until no
    /// remaining candidate can enter the top. Non-applicable knowledge only fills remaining
    /// places when <paramref name="fill"/> is set.
    /// </summary>
    private Selection Select(
        string query,
        IReadOnlyList<string>? kinds,
        Guid? exclude,
        int limit,
        bool fill
    )
    {
        IReadOnlyList<string> terms = SearchTerms.Query(index.Tokenizer, query);
        if (terms.Count == 0)
        {
            throw new SermofurException("invalid_input", "The question holds no searchable term.");
        }
        IReadOnlyList<TermHit> hits = index.Hits(terms, context.VisibleScopes);
        if (hits.Count == 0)
        {
            return new Selection([], 0);
        }
        VisibleCorpus corpus = index.Corpus(context.VisibleScopes);
        Dictionary<string, double> idf = hits.GroupBy(hit => hit.Term, StringComparer.Ordinal)
            .ToDictionary(
                group => group.Key,
                group =>
                {
                    int documents = group.Select(hit => hit.Document).Distinct().Count();
                    return Math.Log(1 + (corpus.Documents - documents + 0.5) / (documents + 0.5));
                },
                StringComparer.Ordinal
            );
        Dictionary<Guid, RecordSummary> records = store
            .ReadSummaries(context.VisibleScopes)
            .ToDictionary(record => record.Id);
        double averageLength = Math.Max(corpus.AverageLength, 1);
        // Best passage of each object, in a deterministic order of relevance.
        Candidate[] candidates = hits.Where(hit => kinds is null || kinds.Contains(hit.Kind))
            .Where(hit => hit.ObjectId != exclude && records.ContainsKey(hit.ObjectId))
            .GroupBy(hit => hit.Document)
            .Select(group =>
            {
                TermHit first = group.First();
                double norm = K1 * (1 - B + B * first.Length / averageLength);
                double relevance = group.Sum(hit =>
                    idf[hit.Term] * hit.Frequency * (K1 + 1) / (hit.Frequency + norm)
                );
                RecordSummary record = records[first.ObjectId];
                return new Candidate(
                    first.Document,
                    record,
                    relevance,
                    group.Select(hit => hit.Term).ToArray(),
                    record.Status is not (KnowledgeStatus.Invalidated or KnowledgeStatus.Superseded)
                );
            })
            .GroupBy(candidate => candidate.Record.Id)
            .Select(group =>
                group.OrderByDescending(c => c.Relevance).ThenBy(c => c.Document).First()
            )
            .OrderByDescending(candidate => candidate.Relevance)
            .ThenByDescending(candidate => candidate.Record.CreatedAt)
            .ThenBy(candidate => candidate.Record.Id)
            .ToArray();
        // Evidence is parsed once. A claim without evidence is low (weight 1); weights are only
        // computed in order of relevance while a candidate can still enter the top (x1.2 bound).
        ILookup<Guid, MemoryRecord> evidence = store
            .ReadRecords(context.VisibleScopes, RecordKind.Evidence)
            .ToLookup(record => RecordJson.Read<EvidenceContent>(record.ContentJson).ClaimId);
        List<(Candidate Candidate, double Score)> top = new List<(Candidate, double)>();
        foreach (Candidate candidate in candidates.Where(c => c.Applicable))
        {
            if (top.Count >= limit && candidate.Relevance * MaxWeight < top[limit - 1].Score)
            {
                break;
            }
            // Bounded insertion: the list never holds more than limit entries, in final order.
            (Candidate Candidate, double Score) entry = (
                candidate,
                candidate.Relevance * Weight(candidate, evidence)
            );
            int position = top.FindIndex(other => Ranks(entry, other) < 0);
            top.Insert(position < 0 ? top.Count : position, entry);
            if (top.Count > limit)
            {
                top.RemoveAt(limit);
            }
        }
        List<RecallResult> shown = top.Select(pair => Describe(pair.Candidate, evidence, terms))
            .ToList();
        Candidate[] notApplicable = candidates.Where(c => !c.Applicable).ToArray();
        if (fill)
        {
            shown.AddRange(
                notApplicable.Take(limit - shown.Count).Select(c => Describe(c, evidence, terms))
            );
        }
        int excluded = notApplicable.Length - shown.Count(result => !result.Applicable);
        return new Selection(shown, fill ? excluded : 0);
    }

    /// <summary>Final order: score, then most recent, then identifier. Negative when a comes first.</summary>
    private static int Ranks(
        (Candidate Candidate, double Score) a,
        (Candidate Candidate, double Score) b
    )
    {
        int order = b.Score.CompareTo(a.Score);
        if (order == 0)
        {
            order = b.Candidate.Record.CreatedAt.CompareTo(a.Candidate.Record.CreatedAt);
        }
        return order != 0 ? order : a.Candidate.Record.Id.CompareTo(b.Candidate.Record.Id);
    }

    private double Weight(Candidate candidate, ILookup<Guid, MemoryRecord> evidence)
    {
        if (candidate.Record.Kind != RecordKind.Claim || !evidence.Contains(candidate.Record.Id))
        {
            return 1.0;
        }
        MemoryRecord claim = Find(candidate.Record.Id);
        return Weight(MemoryService.EvaluateConfidence(claim, evidence[claim.Id].ToArray()).Level);
    }

    /// <summary>Single table of confidence weights, for the ranking and the shown score.</summary>
    private static double Weight(ConfidenceLevel level) =>
        level switch
        {
            ConfidenceLevel.Medium => 1.1,
            ConfidenceLevel.High or ConfidenceLevel.Verified => MaxWeight,
            _ => 1.0,
        };

    private MemoryRecord Find(Guid id) =>
        store.FindRecord(id, context.VisibleScopes)
        ?? throw new SermofurException("storage_busy", "Object changed during recall; retry.", 3);

    /// <summary>Full description of a shown candidate: its record and, for a claim, its evidence.</summary>
    private RecallResult Describe(
        Candidate candidate,
        ILookup<Guid, MemoryRecord> evidence,
        IReadOnlyList<string> terms
    )
    {
        MemoryRecord record = Find(candidate.Record.Id);
        return Describe(
            record,
            evidence[record.Id].ToArray(),
            candidate.Document,
            candidate.Relevance,
            candidate.Terms,
            terms
        );
    }

    private RecallResult Describe(
        MemoryRecord record,
        IReadOnlyList<MemoryRecord> evidence,
        long document,
        double relevance,
        IReadOnlyList<string> matched,
        IReadOnlyList<string> terms
    )
    {
        string excerpt = Excerpt(index.PassageText(document), matched);
        string[] ordered = terms.Where(matched.Contains).ToArray();
        switch (record.Kind)
        {
            case RecordKind.Claim:
                ClaimContent claim = RecordJson.Read<ClaimContent>(record.ContentJson);
                Confidence confidence = MemoryService.EvaluateConfidence(record, evidence);
                bool applicable =
                    record.Status
                    is not (KnowledgeStatus.Invalidated or KnowledgeStatus.Superseded);
                double factor = Weight(confidence.Level);
                return new RecallResult(
                    "claim",
                    record.Id,
                    record.ScopeId,
                    Name(record.Status),
                    applicable,
                    confidence,
                    excerpt,
                    ordered,
                    new Freshness(
                        record.Provenance.Timestamp,
                        claim.LastVerified,
                        claim.ReviewAfter
                    ),
                    Math.Round(relevance * factor, 6)
                );
            case RecordKind.Source:
                SourceContent source = RecordJson.Read<SourceContent>(record.ContentJson);
                return new RecallResult(
                    "source",
                    record.Id,
                    record.ScopeId,
                    Name(record.Status),
                    true,
                    null,
                    excerpt,
                    ordered,
                    new Freshness(
                        record.Provenance.Timestamp,
                        IndexedAt: source.IndexedAt,
                        Hash: source.Hash,
                        SourceStatus: source.Status.ToString().ToLowerInvariant()
                    ),
                    Math.Round(relevance, 6),
                    source.RelativePath
                );
            default:
                return new RecallResult(
                    "retex",
                    record.Id,
                    record.ScopeId,
                    Name(record.Status),
                    true,
                    null,
                    excerpt,
                    ordered,
                    new Freshness(record.Provenance.Timestamp),
                    Math.Round(relevance, 6)
                );
        }
    }

    private static string Name(KnowledgeStatus status) => status.ToString().ToLowerInvariant();

    /// <summary>At most 300 characters around the first matched term, case and accents ignored.</summary>
    private static string Excerpt(string text, IReadOnlyList<string> matched)
    {
        string flat = text.Replace("\r\n", " ", StringComparison.Ordinal).Replace('\n', ' ');
        if (flat.Length <= ExcerptLength)
        {
            return flat;
        }
        CompareInfo compare = CultureInfo.InvariantCulture.CompareInfo;
        int position = matched
            .Select(term =>
                compare.IndexOf(
                    flat,
                    term,
                    CompareOptions.IgnoreCase | CompareOptions.IgnoreNonSpace
                )
            )
            .Where(index => index >= 0)
            .DefaultIfEmpty(0)
            .Min();
        int start = Math.Clamp(position - ExcerptLength / 3, 0, flat.Length - ExcerptLength);
        if (start > 0 && char.IsLowSurrogate(flat[start]))
        {
            start--;
        }
        int length = Math.Min(ExcerptLength, flat.Length - start);
        if (start + length < flat.Length && char.IsHighSurrogate(flat[start + length - 1]))
        {
            length--;
        }
        return (start > 0 ? "…" : string.Empty)
            + flat.Substring(start, length)
            + (start + length < flat.Length ? "…" : string.Empty);
    }
}
