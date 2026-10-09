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

/// <summary>What tells a changed registry file from the one already read.</summary>
public readonly record struct RegistryStamp(DateTime WrittenUtc, long Length)
{
    /// <summary>The stamp of a registry that does not exist.</summary>
    public static RegistryStamp Absent { get; } = new RegistryStamp(DateTime.MinValue, -1);
}

/// <summary>
/// Instances the user entrusted to the daemon (data-model): a small JSON file in the user
/// configuration folder, replaced atomically, never rewritten when it cannot be read.
/// </summary>
public sealed class InstanceRegistry(
    string file,
    InstanceManager? manager = null,
    TimeSpan? lockTimeout = null
)
{
    public const int MaxBytes = 64 * 1024;
    public const int MaxEntries = 256;
    private const int FormatVersion = 1;
    private const int SaveAttempts = 10;

    /// <summary>How long a change waits for another process that changes the registry.</summary>
    public static readonly TimeSpan DefaultLockTimeout = TimeSpan.FromSeconds(10);

    private static readonly TimeSpan LockPoll = TimeSpan.FromMilliseconds(20);
    private static readonly TimeSpan SaveRetryDelay = TimeSpan.FromMilliseconds(50);

    private static readonly JsonSerializerOptions Options = new JsonSerializerOptions
    {
        WriteIndented = true,
    };

    private readonly InstanceManager instances = manager ?? new InstanceManager();
    private readonly TimeSpan lockLimit = lockTimeout ?? DefaultLockTimeout;

    /// <summary>File held while a process changes the registry.</summary>
    public string LockFile => file + ".lock";

    public string File => file;

    /// <summary>Registered instances; an absent file is an empty registry.</summary>
    public IReadOnlyList<RegisteredInstance> Read()
    {
        byte[]? content = ReadContent();
        if (content is null)
        {
            return [];
        }
        try
        {
            RegistryDocument? document = JsonSerializer.Deserialize<RegistryDocument>(content);
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

    /// <summary>
    /// Bytes of the registry, null when absent. Held only for the time of one read, and shared as
    /// widely as the system allows; on Windows a replacement during that instant still fails, and
    /// <see cref="Save"/> tries it again.
    /// </summary>
    private byte[]? ReadContent()
    {
        FileStream stream;
        try
        {
            stream = new FileStream(
                file,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete
            );
        }
        catch (Exception exception)
            when (exception is FileNotFoundException or DirectoryNotFoundException)
        {
            return null;
        }
        using (stream)
        {
            if (stream.Length > MaxBytes)
            {
                throw Invalid($"larger than {MaxBytes} bytes");
            }
            using MemoryStream buffer = new MemoryStream();
            stream.CopyTo(buffer);
            return buffer.ToArray();
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
        using FileStream turn = TakeLock();
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
        using FileStream turn = TakeLock();
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

    /// <summary>
    /// Date of the last write and size of the file, to reload it only when it changed. Two writes
    /// within one tick of the file system share a date: see <see cref="ServingGate"/>.
    /// </summary>
    public RegistryStamp Stamp()
    {
        FileInfo info = new FileInfo(file);
        return info.Exists
            ? new RegistryStamp(info.LastWriteTimeUtc, info.Length)
            : RegistryStamp.Absent;
    }

    /// <summary>Same root, with the case rule of the operating system.</summary>
    public static bool Same(string left, string right) =>
        string.Equals(
            Path.TrimEndingDirectorySeparator(left),
            Path.TrimEndingDirectorySeparator(right),
            LocalPaths.Comparison
        );

    /// <summary>
    /// One change at a time, across processes: two concurrent registrations each keep the
    /// other's entry. Readers do not take it; the file is replaced atomically.
    /// </summary>
    private FileStream TakeLock() =>
        FileTurn.TryTake(LockFile, lockLimit, LockPoll)
        ?? throw Busy($"another smf process has been changing it for {lockLimit.TotalSeconds:0} s");

    /// <summary>
    /// Replaces the file under the lock. On Windows a reader of another process may hold the
    /// file for an instant (an older smf, an antivirus), and the replacement then fails: it is
    /// tried again briefly, then reported as <c>registry_busy</c>, nothing changed. Any other
    /// failure (rights, full disk) is reported as it is, since running again would not help.
    /// </summary>
    private void Save(List<RegisteredInstance> entries)
    {
        byte[] content = JsonSerializer.SerializeToUtf8Bytes(
            new RegistryDocument { Version = FormatVersion, Instances = entries },
            Options
        );
        for (int attempt = 1; ; attempt++)
        {
            try
            {
                AtomicFile.Write(file, content);
                return;
            }
            catch (Exception exception) when (HeldByAReader(exception) && attempt < SaveAttempts)
            {
                Thread.Sleep(SaveRetryDelay);
            }
            catch (Exception exception) when (HeldByAReader(exception))
            {
                throw Busy("the file is held open by another program");
            }
        }
    }

    /// <summary>
    /// A replacement refused because another process has the file open: a sharing or lock
    /// violation, or the access denied that Windows gives when the target of a rename is open.
    /// </summary>
    private bool HeldByAReader(Exception exception)
    {
        if (!OperatingSystem.IsWindows())
        {
            return false;
        }
        const int SharingViolation = unchecked((int)0x80070020);
        const int LockViolation = unchecked((int)0x80070021);
        return exception switch
        {
            // A read-only target refuses the rename for good: that is not a reader.
            UnauthorizedAccessException => System.IO.File.Exists(file)
                && !System.IO.File.GetAttributes(file).HasFlag(FileAttributes.ReadOnly),
            IOException io => io.HResult is SharingViolation or LockViolation,
            _ => false,
        };
    }

    private SermofurException Busy(string reason) =>
        new SermofurException(
            "registry_busy",
            $"The daemon registry {file} is busy ({reason}); nothing changed, run the command again.",
            3
        );

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
