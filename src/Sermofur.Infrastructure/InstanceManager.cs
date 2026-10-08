using System.Diagnostics;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

/// <param name="ownership">Owner and type of entries; the operating system by default.</param>
public sealed class InstanceManager(IFileOwnership? ownership = null)
{
    private readonly IFileOwnership owners = ownership ?? new FileOwnership();

    public const int SchemaVersion = 2;

    /// <summary>Format of the 0.1 CLI: read only by migrate, doctor and root.</summary>
    public const int LegacySchemaVersion = 1;

    /// <summary>Upper bound of <c>instance.json</c>: beyond it the instance is damaged.</summary>
    public const long MaxConfigurationBytes = 64 * 1024;
    public const string Marker = ".sermofur";
    public const string ConfigurationFile = "instance.json";
    public const string DatabaseFile = "memory.db";
    public const string RecordsDirectory = "records";
    public const string BackupsDirectory = "backups";

    /// <summary>Entries that only Sermofur creates inside <c>.sermofur</c>.</summary>
    private static readonly string[] SermofurArtifacts =
    [
        ConfigurationFile,
        DatabaseFile,
        RecordsDirectory,
        BackupsDirectory,
    ];

    public static string DatabasePath(string root) => Path.Combine(root, Marker, DatabaseFile);

    /// <summary>
    /// Case of the <c>.sermofur</c> entry of <paramref name="root"/> when it is not a valid
    /// instance (<c>foreign</c>, <c>damaged</c> or <c>unreadable</c>), null otherwise. Doctor
    /// reports this case, the same classification as discovery, instead of reading the
    /// configuration through a folder that discovery refuses.
    /// </summary>
    public string? DescribeInvalidMarker(string root) =>
        InspectMarker(root) switch
        {
            MarkerState.ForeignOwner => "foreign_owner",
            MarkerState.Foreign => "foreign",
            MarkerState.Damaged => "damaged",
            MarkerState.Unreadable => "unreadable",
            _ => null,
        };

    /// <summary>
    /// Nearest instance root at or above <paramref name="path"/>. Discovery fails closed (ADR
    /// 0010): the nearest <c>.sermofur</c> entry, whatever it is, ends the search. Only a valid
    /// instance is returned; a foreign, damaged or unreadable entry stops with
    /// <c>invalid_instance</c> and a link with <c>unsafe_path</c>, because going on upwards could
    /// silently write into an ancestor instance.
    /// </summary>
    public string Discover(string path) => Locate(path, forDiagnosis: false);

    /// <summary>
    /// Same search as <see cref="Discover"/>, but the directory of an invalid <c>.sermofur</c> entry
    /// is returned so that doctor can report it instead of failing before its report.
    /// </summary>
    public string DiscoverForDiagnosis(string path) => Locate(path, forDiagnosis: true);

    /// <summary>
    /// Reads <c>instance.json</c>. A format 1 instance is refused with <c>migration_required</c>
    /// unless <paramref name="allowLegacy"/> (migrate, doctor). The file is bounded: a larger
    /// one is a damaged instance and is not loaded.
    /// </summary>
    public InstanceConfiguration ReadConfiguration(string root, bool allowLegacy = false)
    {
        LocalPaths.RejectLinks(Path.Combine(root, Marker));
        string file = Path.Combine(root, Marker, ConfigurationFile);
        LocalPaths.RejectLinks(file);
        if (!File.Exists(file))
        {
            throw new SermofurException("invalid_instance", "Instance configuration missing.", 3);
        }
        if (new FileInfo(file).Length > MaxConfigurationBytes)
        {
            throw new SermofurException(
                "invalid_instance",
                "Damaged Sermofur instance: instance.json exceeds 64 KiB; restore it from a backup.",
                3
            );
        }
        InstanceConfiguration configuration = RecordJson.Read<InstanceConfiguration>(
            File.ReadAllText(file)
        );
        if (configuration.InstanceId == Guid.Empty)
        {
            throw new SermofurException("unsupported_schema", "Invalid instance identity.", 3);
        }
        if (configuration.SchemaVersion == LegacySchemaVersion)
        {
            return allowLegacy ? configuration : throw MigrationRequired();
        }
        if (configuration.SchemaVersion != SchemaVersion)
        {
            throw new SermofurException(
                "unsupported_schema",
                "Unsupported instance identity or version.",
                3
            );
        }
        return configuration;
    }

    /// <summary>Single definition of the error for an instance still in format 1.</summary>
    public static SermofurException MigrationRequired() =>
        new(
            "migration_required",
            "Instance in format 1; back it up if you wish, then run smf migrate.",
            3
        );

    public object Initialize(string path)
    {
        string root = LocalPaths.ValidateDirectory(path);
        try
        {
            string existing = Discover(root);
            if (!string.Equals(existing, root, LocalPaths.Comparison))
            {
                throw new SermofurException(
                    "nested_instance",
                    "An ancestor instance exists; nested initialization refused.",
                    4
                );
            }
            InstanceConfiguration config = ReadConfiguration(root);
            using SqliteStore store = new(root, config.InstanceId);
            store.ValidateSchema();
            return new
            {
                root,
                configuration = config,
                created = false,
            };
        }
        catch (SermofurException exception) when (exception.Code == "no_instance") { }

        // no_instance guarantees that neither root nor any ancestor holds a .sermofur entry.
        string staging = Path.Combine(root, $".sermofur-init-{Guid.NewGuid():N}");
        Directory.CreateDirectory(staging);
        InstanceConfiguration configuration = new(
            SchemaVersion,
            Guid.NewGuid(),
            DateTimeOffset.UtcNow
        );
        try
        {
            File.WriteAllText(
                Path.Combine(staging, ConfigurationFile),
                RecordJson.Write(configuration)
            );
            SqliteStore.CreateDatabase(
                Path.Combine(staging, DatabaseFile),
                configuration.InstanceId
            );
            Directory.CreateDirectory(Path.Combine(staging, RecordsDirectory));
            try
            {
                Directory.Move(staging, Path.Combine(root, Marker));
            }
            catch (IOException) when (Directory.Exists(Path.Combine(root, Marker)))
            {
                InstanceConfiguration winner = ReadConfiguration(root);
                using SqliteStore store = new(root, winner.InstanceId);
                store.ValidateSchema();
                return new
                {
                    root,
                    configuration = winner,
                    created = false,
                };
            }
            return new
            {
                root,
                configuration,
                created = true,
            };
        }
        finally
        {
            if (Directory.Exists(staging))
            {
                Directory.Delete(staging, recursive: true);
            }
        }
    }

    /// <summary>
    /// Contextual scope of the current path: the longest mapping that contains it, workspace
    /// otherwise. Mappings are compared lexically; only the selected mapping is checked
    /// physically, so that a mapped directory that disappeared does not block other contexts.
    /// </summary>
    public MemoryContext ResolveContext(string path, string root, IReadOnlyList<Scope> scopes)
    {
        ScopePolicy.ValidateTree(scopes);
        string current = LocalPaths.ValidateDirectory(path);
        MappedScope? selected = scopes
            .Where(scope => scope.RelativePath is not null)
            .Select(scope => new MappedScope(
                scope,
                LocalPaths.NormalizeMapping(root, scope.RelativePath!)
            ))
            .Where(mapping => LocalPaths.Contains(mapping.Path, current))
            .OrderByDescending(mapping => mapping.Path.Length)
            .ThenByDescending(mapping => mapping.Scope.Kind)
            .FirstOrDefault();
        string scopeId = ScopePolicy.WorkspaceScopeId;
        if (selected is not null)
        {
            LocalPaths.ValidateDirectory(selected.Path);
            scopeId = selected.Scope.Id;
        }
        return new MemoryContext(root, scopeId, ScopePolicy.VisibleAncestors(scopeId, scopes));
    }

    private string Locate(string path, bool forDiagnosis)
    {
        string? current = LocalPaths.ValidateDirectory(path);
        while (current is not null)
        {
            MarkerState state = InspectMarker(current);
            if (state == MarkerState.Instance || (state != MarkerState.Absent && forDiagnosis))
            {
                return current;
            }
            if (state != MarkerState.Absent)
            {
                throw InvalidMarker(state);
            }
            current = Path.GetDirectoryName(current);
        }
        throw new SermofurException(
            "no_instance",
            "No .sermofur instance in the parent folders.",
            2
        );
    }

    /// <summary>
    /// Classifies the <c>.sermofur</c> entry of <paramref name="directory"/> without following
    /// links, so that a dangling link is seen as an entry and not as an absence. Any link is
    /// refused, even for doctor. An entry whose attributes or listing cannot be read is
    /// unreadable, never absent nor foreign: when the attributes fail, even the presence of the
    /// entry is unknown, and failing closed is the only safe answer.
    /// </summary>
    private MarkerState InspectMarker(string directory)
    {
        string marker = Path.Combine(directory, Marker);
        FileAttributes attributes;
        try
        {
            attributes = File.GetAttributes(marker);
        }
        catch (Exception exception)
            when (exception is FileNotFoundException or DirectoryNotFoundException)
        {
            return MarkerState.Absent;
        }
        catch (Exception exception) when (exception is UnauthorizedAccessException or IOException)
        {
            return MarkerState.Unreadable;
        }
        if ((attributes & FileAttributes.ReparsePoint) != 0)
        {
            throw LocalPaths.LinkNotSupported();
        }
        if ((attributes & FileAttributes.Directory) == 0)
        {
            return MarkerState.Foreign;
        }
        // An entry of another account is never read (ADR 0014): its content could be planted.
        try
        {
            if (owners.Inspect(marker) is { IsOwnedByCurrentUser: false })
            {
                return MarkerState.ForeignOwner;
            }
        }
        catch (Exception exception) when (exception is UnauthorizedAccessException or IOException)
        {
            return MarkerState.Unreadable;
        }
        HashSet<string> entries;
        try
        {
            entries = Directory
                .EnumerateFileSystemEntries(marker)
                .Select(entry => Path.GetFileName(entry))
                .ToHashSet(StringComparer.FromComparison(LocalPaths.Comparison));
        }
        catch (Exception exception) when (exception is UnauthorizedAccessException or IOException)
        {
            return MarkerState.Unreadable;
        }
        if (entries.Contains(ConfigurationFile))
        {
            return MarkerState.Instance;
        }
        return SermofurArtifacts.Any(entries.Contains) ? MarkerState.Damaged : MarkerState.Foreign;
    }

    private static SermofurException InvalidMarker(MarkerState state) =>
        state switch
        {
            MarkerState.ForeignOwner => new(
                "foreign_owner",
                "The .sermofur entry belongs to another user account; it is not read. Use your own instance.",
                4
            ),
            MarkerState.Damaged => new(
                "invalid_instance",
                "Damaged Sermofur instance: .sermofur holds Sermofur data but no instance.json; run smf doctor and restore instance.json from a backup.",
                3
            ),
            MarkerState.Unreadable => new(
                "invalid_instance",
                "A .sermofur entry, or whether one exists, cannot be read in this folder or a parent; check permissions, then run smf doctor.",
                3
            ),
            MarkerState.Foreign => new(
                "invalid_instance",
                "A .sermofur entry that is not a Sermofur instance stops the search for an instance; rename or move it away.",
                3
            ),
            _ => throw new UnreachableException(
                $"Marker state {state} does not describe an invalid entry."
            ),
        };

    private enum MarkerState
    {
        Absent,
        Foreign,
        Damaged,
        Unreadable,
        ForeignOwner,
        Instance,
    }

    private sealed record MappedScope(Scope Scope, string Path);
}
