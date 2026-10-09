using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>
/// Which registered instance serves a path (research R8). The path must lie under a registered
/// root, compared as strings without touching the disk; then the usual fail-closed discovery,
/// which never climbs above that root, must land on the same root. Anything else is not served
/// and the client runs the command directly, with the errors of the direct CLI.
/// </summary>
public sealed class ServingGate(
    InstanceRegistry registry,
    InstanceManager? manager = null,
    TimeSpan? idle = null,
    Func<DateTimeOffset>? clock = null,
    Action<string>? refused = null
) : IServingGate
{
    private readonly InstanceManager instances = manager ?? new InstanceManager();
    private readonly TimeSpan idleLimit = idle ?? TimeSpan.FromMinutes(10);
    private readonly Func<DateTimeOffset> now = clock ?? (() => DateTimeOffset.Now);
    private readonly object gate = new();
    private readonly Dictionary<string, DateTimeOffset> open = new(
        OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal
    );

    /// <summary>
    /// Longer than the coarsest date of a file system in use (FAT: 2 s), so that two writes with
    /// the same date and size are never both missed.
    /// </summary>
    private static readonly TimeSpan RecentWrite = TimeSpan.FromSeconds(3);

    private IReadOnlyList<RegisteredInstance> entries = [];

    /// <summary>Stamp of the registry as last read; null before the first read.</summary>
    private RegistryStamp? loaded;
    private bool recent;

    public int OpenCount
    {
        get
        {
            lock (gate)
            {
                CloseIdle();
                return open.Count;
            }
        }
    }

    public string? Resolve(string path)
    {
        RegisteredInstance? candidate;
        lock (gate)
        {
            Reload();
            candidate = entries
                .Where(entry => LocalPaths.Contains(entry.Root, path))
                .OrderByDescending(entry => entry.Root.Length)
                .FirstOrDefault();
        }
        if (candidate is null)
        {
            refused?.Invoke("not_registered");
            return null;
        }
        string root;
        try
        {
            root = instances.Discover(path);
        }
        catch (SermofurException exception)
        {
            // The direct CLI reports the same error with its own wording and exit code.
            refused?.Invoke(exception.Code);
            return null;
        }
        if (!InstanceRegistry.Same(root, candidate.Root))
        {
            refused?.Invoke("nested_instance_not_registered");
            // A nested instance that is not registered itself.
            return null;
        }
        lock (gate)
        {
            open[candidate.Root] = now();
        }
        return candidate.Root;
    }

    private void Reload()
    {
        RegistryStamp stamp = registry.Stamp();
        if (stamp == loaded && !recent)
        {
            return;
        }
        try
        {
            entries = registry.Read();
        }
        catch (SermofurException exception)
        {
            // An unreadable registry serves nothing; the CLI keeps working directly.
            refused?.Invoke(exception.Code);
            entries = [];
        }
        loaded = stamp;
        // A file written within the last ticks of the file system may be written again with the
        // same date and size: read it again next time, until it has aged. A date in the future
        // (clock set back, restored file) counts as aged: the stamp alone decides.
        TimeSpan age = DateTime.UtcNow - stamp.WrittenUtc;
        recent = age >= TimeSpan.Zero && age < RecentWrite;
        foreach (string root in open.Keys.ToList())
        {
            if (!entries.Any(entry => InstanceRegistry.Same(entry.Root, root)))
            {
                open.Remove(root);
            }
        }
    }

    private void CloseIdle()
    {
        DateTimeOffset limit = now() - idleLimit;
        foreach (KeyValuePair<string, DateTimeOffset> pair in open.ToList())
        {
            if (pair.Value < limit)
            {
                open.Remove(pair.Key);
            }
        }
    }
}
