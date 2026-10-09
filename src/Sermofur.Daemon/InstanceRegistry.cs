using System.Text.Json;
using System.Text.Json.Serialization;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>One instance the daemon may serve.</summary>
public sealed record RegisteredInstance(
    [property: JsonPropertyName("root")] string Root,
    [property: JsonPropertyName("registeredAt")] string RegisteredAt
);

/// <summary>An entry of the registry with whether its instance is still where it was registered.</summary>
public sealed record RegistryEntry(string Root, string RegisteredAt, bool Missing);

/// <summary>
/// Instances the user entrusted to the daemon (data-model): a small JSON file in the user
/// configuration folder, replaced atomically, never rewritten when it cannot be read.
/// </summary>
public sealed class InstanceRegistry(string file, InstanceManager? manager = null)
{
    public const int MaxBytes = 64 * 1024;
    public const int MaxEntries = 256;
    private const int FormatVersion = 1;

    private static readonly JsonSerializerOptions Options = new JsonSerializerOptions
    {
        WriteIndented = true,
    };

    private readonly InstanceManager instances = manager ?? new InstanceManager();

    public string File => file;

    /// <summary>Registered instances; an absent file is an empty registry.</summary>
    public IReadOnlyList<RegisteredInstance> Read()
    {
        FileInfo info = new FileInfo(file);
        if (!info.Exists)
        {
            return [];
        }
        if (info.Length > MaxBytes)
        {
            throw Invalid($"larger than {MaxBytes} bytes");
        }
        try
        {
            RegistryDocument? document = JsonSerializer.Deserialize<RegistryDocument>(
                System.IO.File.ReadAllBytes(file)
            );
            if (
                document?.Instances is null
                || document.Version != FormatVersion
                || document.Instances.Count > MaxEntries
                || document.Instances.Any(entry =>
                    string.IsNullOrEmpty(entry.Root) || !Path.IsPathFullyQualified(entry.Root)
                )
            )
            {
                throw Invalid("unexpected content");
            }
            return document.Instances;
        }
        catch (JsonException)
        {
            throw Invalid("malformed JSON");
        }
    }

    /// <summary>Entries with a flag for instances no longer found at their root.</summary>
    public IReadOnlyList<RegistryEntry> List() =>
        Read()
            .Select(entry => new RegistryEntry(
                entry.Root,
                entry.RegisteredAt,
                !System.IO.File.Exists(
                    Path.Combine(
                        entry.Root,
                        InstanceManager.Marker,
                        InstanceManager.ConfigurationFile
                    )
                )
            ))
            .ToList();

    /// <summary>
    /// Registers the instance found from <paramref name="path"/>, with the checks of a direct
    /// command (format, owner, integrity) and their error codes; idempotent.
    /// </summary>
    public RegisteredInstance Register(string path, DateTimeOffset now)
    {
        string root = instances.Discover(path);
        instances.ReadConfiguration(root);
        List<RegisteredInstance> entries = [.. Read()];
        RegisteredInstance? existing = entries.FirstOrDefault(entry => Same(entry.Root, root));
        if (existing is not null)
        {
            return existing;
        }
        if (entries.Count >= MaxEntries)
        {
            throw Invalid($"more than {MaxEntries} instances");
        }
        RegisteredInstance added = new RegisteredInstance(
            root,
            now.ToString("yyyy-MM-dd'T'HH:mm:sszzz")
        );
        entries.Add(added);
        Save(entries);
        return added;
    }

    /// <summary>
    /// Removes the instance registered at <paramref name="path"/>, or the one found from it;
    /// <c>not_registered</c> when neither is in the registry.
    /// </summary>
    public RegisteredInstance Unregister(string path)
    {
        List<RegisteredInstance> entries = [.. Read()];
        string full = Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
        RegisteredInstance? match = entries.FirstOrDefault(entry => Same(entry.Root, full));
        if (match is null)
        {
            try
            {
                string root = instances.Discover(full);
                match = entries.FirstOrDefault(entry => Same(entry.Root, root));
            }
            catch (SermofurException)
            {
                // A moved or deleted instance is removed by the exact path it was registered at.
            }
        }
        if (match is null)
        {
            throw new SermofurException(
                "not_registered",
                $"No registered instance at {full}; see: smf daemon instances",
                1
            );
        }
        entries.Remove(match);
        Save(entries);
        return match;
    }

    /// <summary>Last change of the file, to reload it only when it changed.</summary>
    public DateTime LastWriteUtc() =>
        System.IO.File.Exists(file) ? System.IO.File.GetLastWriteTimeUtc(file) : DateTime.MinValue;

    /// <summary>Same root, with the case rule of the operating system.</summary>
    public static bool Same(string left, string right) =>
        string.Equals(
            Path.TrimEndingDirectorySeparator(left),
            Path.TrimEndingDirectorySeparator(right),
            LocalPaths.Comparison
        );

    private void Save(List<RegisteredInstance> entries)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        byte[] content = JsonSerializer.SerializeToUtf8Bytes(
            new RegistryDocument { Version = FormatVersion, Instances = entries },
            Options
        );
        AtomicFile.Write(file, content);
    }

    private SermofurException Invalid(string reason) =>
        new SermofurException(
            "invalid_registry",
            $"The daemon registry {file} cannot be used ({reason}); it is left untouched.",
            3
        );

    private sealed class RegistryDocument
    {
        [JsonPropertyName("version")]
        public int Version { get; init; }

        [JsonPropertyName("instances")]
        public List<RegisteredInstance>? Instances { get; init; }
    }
}
