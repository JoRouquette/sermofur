using Sermofur.Domain;

namespace Sermofur.Infrastructure;

/// <summary>What a write does when a link stands where its target should be.</summary>
public enum LinkPolicy
{
    /// <summary>
    /// A link in place of the target is refused. The state of sermofur is written this way: no
    /// link there was put by the product, and replacing it would write outside its own folder.
    /// Only a symbolic link and a junction are seen; a hard link is another name of the same
    /// file, which no attribute tells apart, so a write through one replaces that name alone.
    /// </summary>
    Refuse,

    /// <summary>
    /// The chain of links is followed and its final target is replaced. The configuration of a
    /// host is written this way: a manager of dotfiles commonly links it to a repository, and
    /// writing over the link would silently take the file out of that repository. The chain is
    /// followed wherever it leads, so the caller names the file it ended on (ADR 0016).
    /// </summary>
    FollowToTarget,
}

/// <summary>Everything a write decides beyond the path and the content.</summary>
/// <param name="NewFileMode">
/// Mode of a file that does not exist yet, on Unix. A file that exists keeps its own mode.
/// </param>
/// <param name="Policy">
/// What a link in place of the target means. <see cref="LinkPolicy.Refuse"/> by default, the
/// right one for the state of the product.
/// </param>
/// <param name="Durable">
/// Whether the content reaches the disk before the rename. True for a file that is an authority
/// of its own; false for one rebuilt from elsewhere, where the cost of a flush buys nothing.
/// </param>
public sealed record WriteOptions(
    UnixFileMode? NewFileMode = null,
    LinkPolicy Policy = LinkPolicy.Refuse,
    bool Durable = true
);

/// <summary>
/// Replaces a file in one step: a temporary file of a random name, created new in the same folder
/// (never through an existing link), then renamed over the target. On Unix the target keeps its
/// permissions; a new file gets <see cref="WriteOptions.NewFileMode"/> when given.
/// <see cref="LinkPolicy"/> decides what a link in place of the target means. Only the target is
/// looked at, not the folders above it: a folder of the path may well be a link on a healthy
/// system (<c>/var</c> is one on macOS).
/// </summary>
public static class AtomicFile
{
    /// <summary>Links followed at most, past which a chain is refused as too long.</summary>
    private const int MaximumLinks = 10;

    /// <summary>Replaces <paramref name="file"/> with <paramref name="content"/>.</summary>
    /// <param name="file">Path asked for, which a followed link may move elsewhere.</param>
    /// <param name="content">Bytes the file ends up holding.</param>
    /// <param name="options">Mode, link policy and durability; the defaults are the strict ones.</param>
    /// <returns>
    /// The file actually replaced. It differs from <paramref name="file"/> when a link was
    /// followed, and a caller that reports what it wrote names this path, not the one asked for.
    /// </returns>
    public static string Write(string file, byte[] content, WriteOptions? options = null)
    {
        WriteOptions settings = options ?? new WriteOptions();
        string target = Target(file, settings.Policy);
        string folder = Path.GetDirectoryName(target)!;
        Directory.CreateDirectory(folder);
        UnixFileMode? mode = null;
        if (!OperatingSystem.IsWindows())
        {
            mode = File.Exists(target) ? File.GetUnixFileMode(target) : settings.NewFileMode;
        }
        string temporary = Path.Combine(
            folder,
            $".{Path.GetFileName(target)}.{Path.GetRandomFileName()}.tmp"
        );
        try
        {
            FileStreamOptions streamOptions = new FileStreamOptions
            {
                Mode = FileMode.CreateNew,
                Access = FileAccess.Write,
                Share = FileShare.None,
            };
            if (mode is UnixFileMode born && !OperatingSystem.IsWindows())
            {
                // Born with the final mode (the umask can only narrow it): a private content is
                // never readable by others, not even before the rename.
                streamOptions.UnixCreateMode = born;
            }
            using (FileStream stream = new FileStream(temporary, streamOptions))
            {
                stream.Write(content);
                stream.Flush(settings.Durable);
            }
            if (mode is UnixFileMode kept && !OperatingSystem.IsWindows())
            {
                // Undoes the umask, so the file ends with exactly the mode it had.
                File.SetUnixFileMode(temporary, kept);
            }
            File.Move(temporary, target, overwrite: true);
            return target;
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
        }
    }

    /// <summary>The file the write ends up replacing, once the policy has had its say.</summary>
    private static string Target(string file, LinkPolicy policy)
    {
        string full = Path.GetFullPath(file);
        return policy == LinkPolicy.FollowToTarget ? FinalTarget(full) : Unlinked(full);
    }

    /// <summary>The path itself, as long as no link stands there.</summary>
    private static string Unlinked(string full)
    {
        if (new FileInfo(full).LinkTarget is not null)
        {
            throw Refused(full, "it is a link, and the write would replace the link itself");
        }
        if (Directory.Exists(full))
        {
            throw Refused(full, "it is a folder");
        }
        return full;
    }

    /// <summary>
    /// End of the chain of links that starts at <paramref name="full"/>: the file a write there
    /// should reach. A link that leads nowhere, to a folder, in a circle or through too many
    /// steps is refused by its own reason, because writing anyway would create a file the host
    /// would not read.
    /// </summary>
    private static string FinalTarget(string full)
    {
        string current = full;
        HashSet<string> seen = new HashSet<string>(
            StringComparer.FromComparison(LocalPaths.Comparison)
        )
        {
            current,
        };
        for (int followed = 0; followed <= MaximumLinks; followed++)
        {
            string? link = new FileInfo(current).LinkTarget;
            if (link is null)
            {
                return EndOfChain(full, current, followed);
            }
            // The target of a link may be written relative to the folder holding the link.
            current = Path.GetFullPath(link, Path.GetDirectoryName(current)!);
            if (!seen.Add(current))
            {
                throw Refused(full, $"its links come back to {current}, in a circle");
            }
        }
        throw Refused(full, $"its links go through more than {MaximumLinks} steps");
    }

    /// <summary>The file a chain of links ends on, when a write may reach it.</summary>
    private static string EndOfChain(string full, string current, int followed)
    {
        if (Directory.Exists(current))
        {
            throw Refused(
                full,
                followed == 0 ? "it is a folder" : $"its links end on the folder {current}"
            );
        }
        if (followed > 0 && !File.Exists(current))
        {
            throw Refused(full, $"its links end on {current}, which does not exist");
        }
        return current;
    }

    private static SermofurException Refused(string file, string why) =>
        new SermofurException("unsafe_path", $"{file} was left untouched: {why}.", 4);
}
