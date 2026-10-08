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
        if (bytes.Length > SourceLimits.MaxBytes)
        {
            return Rejected(SourceRejection.TooLarge, string.Empty, bytes.Length);
        }
        // The hash covers exactly the bytes read, those that are indexed.
        string hash = Convert.ToHexStringLower(SHA256.HashData(bytes));
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
            return Rejected(SourceRejection.Binary, hash, bytes.Length);
        }
        if (text.Contains('\0'))
        {
            return Rejected(SourceRejection.Binary, hash, bytes.Length);
        }
        if (string.IsNullOrWhiteSpace(text))
        {
            return Rejected(SourceRejection.Empty, hash, bytes.Length);
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
        // Sized on the file, never more than the bound plus one byte; the loop stays bounded even
        // if the file grows while it is read.
        int capacity = (int)Math.Min(stream.Length, SourceLimits.MaxBytes) + 1;
        byte[] buffer = new byte[capacity];
        int total = 0;
        int read;
        while (
            total < buffer.Length && (read = stream.Read(buffer, total, buffer.Length - total)) > 0
        )
        {
            total += read;
        }
        if (total == buffer.Length && total <= SourceLimits.MaxBytes)
        {
            // The file grew past its announced length: read on, up to the bound plus one byte.
            Array.Resize(ref buffer, SourceLimits.MaxBytes + 1);
            while (
                total < buffer.Length
                && (read = stream.Read(buffer, total, buffer.Length - total)) > 0
            )
            {
                total += read;
            }
        }
        return total == buffer.Length ? buffer : buffer[..total];
    }

    private static SourceSnapshot Empty(SourceStatus status) => new(status, string.Empty, 0, []);

    private static SourceSnapshot Rejected(
        SourceRejection rejection,
        string hash = "",
        long size = 0
    ) => new(SourceStatus.Rejected, hash, size, [], rejection);
}
