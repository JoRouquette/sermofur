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
        LocalPaths.RejectLinks(target);
        Directory.CreateDirectory(Path.GetDirectoryName(target)!);
        string temporary = target + $".{Guid.NewGuid():N}.tmp";
        try
        {
            File.WriteAllText(temporary, Render(record), new UTF8Encoding(false));
            File.Move(temporary, target, overwrite: true);
        }
        catch (Exception exception)
            when ((exception is IOException or UnauthorizedAccessException) && IsCurrentSafe(record)
            )
        {
            // A concurrent writer already published exactly the same projection.
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
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
