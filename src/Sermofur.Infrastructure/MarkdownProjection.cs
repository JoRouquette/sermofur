using System.Security.Cryptography;
using System.Text;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

public sealed class MarkdownProjection(string root)
{
    public string PathFor(MemoryRecord record) =>
        Path.Combine(
            root,
            InstanceManager.Marker,
            InstanceManager.RecordsDirectory,
            record.Id + ".md"
        );

    public static string Render(MemoryRecord record)
    {
        string metadata = RecordJson.Write(
            new
            {
                schemaVersion = 1,
                record.Id,
                record.ScopeId,
                record.Kind,
                record.Status,
                record.Revision,
                record.Provenance,
            }
        );
        return $"---\n{metadata}\n---\n\n# {record.Kind} {record.Id}\n\n```json\n{record.ContentJson}\n```\n";
    }

    public void Write(MemoryRecord record)
    {
        string target = PathFor(record);
        // Refused as a link on the whole path (ADR 0010), as the configuration of the instance
        // is; AtomicFile refuses one in place of the file itself.
        LocalPaths.RejectLinks(target);
        try
        {
            // A projection is rebuilt from the store, which is the authority: a flush to the
            // disk for each record would buy nothing.
            AtomicFile.Write(
                target,
                Encoding.UTF8.GetBytes(Render(record)),
                new WriteOptions(Durable: false)
            );
        }
        catch (Exception exception)
            when ((exception is IOException or UnauthorizedAccessException) && IsCurrentSafe(record)
            )
        {
            // A concurrent writer already published exactly the same projection.
        }
    }

    public bool IsCurrent(MemoryRecord record)
    {
        string target = PathFor(record);
        LocalPaths.RejectLinks(target);
        return File.Exists(target) && File.ReadAllText(target) == Render(record);
    }

    public bool IsCurrentSafe(MemoryRecord record)
    {
        try
        {
            return IsCurrent(record);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    public static string Hash(string content) =>
        Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(content)));
}
