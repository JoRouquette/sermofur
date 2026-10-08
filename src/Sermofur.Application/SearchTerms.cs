using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>Splits a text into the terms of the search index, by the index's own tokenizer.</summary>
public interface ITokenizer
{
    IReadOnlyList<string> Tokenize(string text);
}

/// <summary>
/// Query terms. The question is always plain text: it is split by the index tokenizer, never
/// parsed as query syntax.
/// </summary>
public static class SearchTerms
{
    /// <summary>Maximum number of distinct terms kept from a query.</summary>
    public const int MaxQueryTerms = 32;

    /// <summary>Distinct terms of a query, in their order of appearance, at most 32.</summary>
    public static IReadOnlyList<string> Query(ITokenizer tokenizer, string query) =>
        tokenizer.Tokenize(query).Distinct(StringComparer.Ordinal).Take(MaxQueryTerms).ToArray();

    /// <summary>
    /// FTS5 expression matching any of the terms. Each term is a quoted string with its quotes
    /// doubled, so that no operator, column filter or prefix syntax can reach the engine.
    /// </summary>
    public static string MatchExpression(IReadOnlyList<string> terms)
    {
        if (terms.Count == 0)
        {
            throw new SermofurException("invalid_input", "The question holds no searchable term.");
        }
        return string.Join(
            " OR ",
            terms.Select(term => "\"" + term.Replace("\"", "\"\"", StringComparison.Ordinal) + "\"")
        );
    }
}

/// <summary>A searchable unit of an object: the whole text of a claim or RETEX, a source passage.</summary>
public sealed record SearchDocument(int Passage, string Text);

public static class SearchExpectations
{
    /// <summary>True if the object must have entries in the search index.</summary>
    public static bool IsIndexed(MemoryRecord record) =>
        record.Kind switch
        {
            RecordKind.Claim or RecordKind.Retex => true,
            RecordKind.Source => RecordJson.Read<SourceContent>(record.ContentJson).Status
                == SourceStatus.Indexed,
            _ => false,
        };
}

public static class SearchDocuments
{
    /// <summary>Documents of a claim or a RETEX; sources are indexed from their file.</summary>
    public static IReadOnlyList<SearchDocument> For(MemoryRecord record) =>
        record.Kind switch
        {
            RecordKind.Claim => [new(0, RecordJson.Read<ClaimContent>(record.ContentJson).Text)],
            RecordKind.Retex =>
            [
                new(0, RetexText(RecordJson.Read<RetexContent>(record.ContentJson))),
            ],
            _ => [],
        };

    private static string RetexText(RetexContent retex) =>
        retex.Event + "\n" + retex.Impact + "\n" + retex.NextAction;
}
