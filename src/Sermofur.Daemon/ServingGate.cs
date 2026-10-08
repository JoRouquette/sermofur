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
    Func<DateTimeOffset>? clock = null
) : IServingGate
{
    private readonly InstanceManager instances = manager ?? new InstanceManager();
    private readonly TimeSpan idleLimit = idle ?? TimeSpan.FromMinutes(10);
    private readonly Func<DateTimeOffset> now = clock ?? (() => DateTimeOffset.Now);
    private readonly object gate = new();
    private readonly Dictionary<string, DateTimeOffset> open = new(
        OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal
    );
    private IReadOnlyList<RegisteredInstance> entries = [];
    private DateTime loadedAt = DateTime.MinValue;

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
            return null;
        }
        string root;
        try
        {
            root = instances.Discover(path);
        }
        catch (SermofurException)
        {
            // The direct CLI reports the same error with its own wording and exit code.
            return null;
        }
        if (!InstanceRegistry.Same(root, candidate.Root))
        {
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
        DateTime written = registry.LastWriteUtc();
        if (written == loadedAt)
        {
            return;
        }
        try
        {
            entries = registry.Read();
        }
        catch (SermofurException)
        {
            // An unreadable registry serves nothing; the CLI keeps working directly.
            entries = [];
        }
        loadedAt = written;
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
