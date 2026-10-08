using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>Bounds of a source file.</summary>
public static class SourceLimits
{
    /// <summary>Maximum size of an indexed source: 1 MiB (clarification of 2026-10-08).</summary>
    public const int MaxBytes = 1024 * 1024;

    /// <summary>Maximum length of a passage of the search index.</summary>
    public const int MaxPassageLength = 2_000;
}

/// <summary>What a reading of a source file found; passages only when indexable.</summary>
public sealed record SourceSnapshot(
    SourceStatus Status,
    string Hash,
    long Size,
    IReadOnlyList<SearchDocument> Passages,
    SourceRejection? Rejection = null
);

/// <summary>Reads a declared source file: one bounded read, its hash and its passages.</summary>
public interface ISourceReader
{
    /// <summary>
    /// Reads <paramref name="relativePath"/> (stored form, <c>/</c> separators) below
    /// <paramref name="root"/>. Never throws for a missing, unreadable or invalid file: the
    /// snapshot says so.
    /// </summary>
    SourceSnapshot Read(string root, string relativePath);
}

/// <summary>Outcome of a reindexing, per source.</summary>
public enum ReindexOutcome
{
    Unchanged,
    Modified,
    Missing,
    Unreadable,
    Rejected,
    Restored,

    /// <summary>Changed by another command since it was listed: left as that command wrote it.</summary>
    Skipped,
}

public sealed record ReindexEntry(
    Guid Id,
    string Path,
    ReindexOutcome Outcome,
    string? PreviousHash,
    string Hash
);

public static class SourcePassages
{
    /// <summary>
    /// Splits a text into passages of at most <see cref="SourceLimits.MaxPassageLength"/>
    /// characters: paragraphs (separated by blank lines) are grouped while they fit; a longer
    /// paragraph is cut on line ends, then on the length bound as a last resort.
    /// </summary>
    public static IReadOnlyList<SearchDocument> Split(string text)
    {
        string normalized = text.Replace("\r\n", "\n", StringComparison.Ordinal);
        List<string> pieces = new List<string>();
        foreach (
            string paragraph in normalized.Split("\n\n", StringSplitOptions.RemoveEmptyEntries)
        )
        {
            pieces.AddRange(Cut(paragraph.Trim('\n')));
        }
        List<SearchDocument> passages = new List<SearchDocument>();
        string current = string.Empty;
        foreach (string piece in pieces.Where(piece => !string.IsNullOrWhiteSpace(piece)))
        {
            if (
                current.Length > 0
                && current.Length + 2 + piece.Length > SourceLimits.MaxPassageLength
            )
            {
                passages.Add(new SearchDocument(passages.Count, current));
                current = string.Empty;
            }
            current = current.Length == 0 ? piece : current + "\n\n" + piece;
        }
        if (current.Length > 0)
        {
            passages.Add(new SearchDocument(passages.Count, current));
        }
        return passages;
    }

    private static IEnumerable<string> Cut(string paragraph)
    {
        if (paragraph.Length <= SourceLimits.MaxPassageLength)
        {
            yield return paragraph;
            yield break;
        }
        string current = string.Empty;
        foreach (string line in paragraph.Split('\n'))
        {
            foreach (string part in Chunk(line))
            {
                if (
                    current.Length > 0
                    && current.Length + 1 + part.Length > SourceLimits.MaxPassageLength
                )
                {
                    yield return current;
                    current = string.Empty;
                }
                current = current.Length == 0 ? part : current + "\n" + part;
            }
        }
        if (current.Length > 0)
        {
            yield return current;
        }
    }

    private static IEnumerable<string> Chunk(string line)
    {
        int start = 0;
        while (start < line.Length)
        {
            int length = Math.Min(SourceLimits.MaxPassageLength, line.Length - start);
            // Never split a surrogate pair.
            if (start + length < line.Length && char.IsHighSurrogate(line[start + length - 1]))
            {
                length--;
            }
            yield return line.Substring(start, length);
            start += length;
        }
    }
}
