using System.Security.Cryptography;
using System.Text;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

/// <summary>
/// Reads a source file once, bounded to 1 MiB + 1 byte: the hash describes exactly the bytes
/// indexed. Strict UTF-8 (a BOM is accepted); a NUL byte or invalid UTF-8 is binary.
/// </summary>
public sealed class FileSourceReader(IFileOwnership ownership) : ISourceReader
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public SourceSnapshot Read(string root, string relativePath)
    {
        string full;
        try
        {
            full = LocalPaths.NormalizeMapping(root, relativePath);
            LocalPaths.RejectLinks(full);
        }
        catch (SermofurException exception)
            when (exception.Code is "unsafe_path" or "scope_boundary")
        {
            return Rejected(SourceRejection.UnsafePath);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            return Empty(SourceStatus.Unreadable);
        }
        EntryStatus? status;
        try
        {
            status = ownership.Inspect(full);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            return Empty(SourceStatus.Unreadable);
        }
        if (status is null || status.IsDirectory)
        {
            return Empty(SourceStatus.Missing);
        }
        if (!status.IsRegularFile)
        {
            // FIFO, device or socket: opening it could block or never end.
            return Rejected(SourceRejection.Binary);
        }
        byte[] bytes;
        try
        {
            bytes = ReadBounded(full);
        }
        catch (Exception exception)
            when (exception is FileNotFoundException or DirectoryNotFoundException)
        {
            return Empty(SourceStatus.Missing);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            return Empty(SourceStatus.Unreadable);
        }
        return Snapshot(bytes);
    }

    internal static SourceSnapshot Snapshot(byte[] bytes)
    {
        string hash = Convert.ToHexStringLower(SHA256.HashData(bytes));
        if (bytes.Length > SourceLimits.MaxBytes)
        {
            return new SourceSnapshot(
                SourceStatus.Rejected,
                string.Empty,
                bytes.Length,
                [],
                SourceRejection.TooLarge
            );
        }
        ReadOnlySpan<byte> content = bytes;
        if (content.StartsWith(Encoding.UTF8.Preamble))
        {
            content = content[Encoding.UTF8.Preamble.Length..];
        }
        string text;
        try
        {
            text = StrictUtf8.GetString(content);
        }
        catch (DecoderFallbackException)
        {
            return new SourceSnapshot(
                SourceStatus.Rejected,
                hash,
                bytes.Length,
                [],
                SourceRejection.Binary
            );
        }
        if (text.Contains('\0'))
        {
            return new SourceSnapshot(
                SourceStatus.Rejected,
                hash,
                bytes.Length,
                [],
                SourceRejection.Binary
            );
        }
        if (string.IsNullOrWhiteSpace(text))
        {
            return new SourceSnapshot(
                SourceStatus.Rejected,
                hash,
                bytes.Length,
                [],
                SourceRejection.Empty
            );
        }
        return new SourceSnapshot(
            SourceStatus.Indexed,
            hash,
            bytes.Length,
            SourcePassages.Split(text)
        );
    }

    private static byte[] ReadBounded(string path)
    {
        using FileStream stream = new FileStream(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete
        );
        byte[] buffer = new byte[SourceLimits.MaxBytes + 1];
        int total = 0;
        int read;
        while (
            total < buffer.Length && (read = stream.Read(buffer, total, buffer.Length - total)) > 0
        )
        {
            total += read;
        }
        return buffer[..total];
    }

    private static SourceSnapshot Empty(SourceStatus status) => new(status, string.Empty, 0, []);

    private static SourceSnapshot Rejected(SourceRejection rejection) =>
        new(SourceStatus.Rejected, string.Empty, 0, [], rejection);
}
