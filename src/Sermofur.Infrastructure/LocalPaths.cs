using System.Text;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

public static class LocalPaths
{
    /// <summary>Separator of a stored mapping, whatever the operating system.</summary>
    public const char MappingSeparator = '/';

    public static StringComparison Comparison =>
        OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

    public static string ValidateDirectory(string path)
    {
        string full = Path.GetFullPath(path);
        if (full.StartsWith("\\\\", StringComparison.Ordinal) || IsNetworkDrive(full))
        {
            throw new SermofurException("unsafe_path", "An instance requires a local disk.", 4);
        }
        if (!Directory.Exists(full))
        {
            throw new SermofurException("invalid_path", "The context directory must exist.");
        }
        RejectLinks(full);
        return Path.TrimEndingDirectorySeparator(full);
    }

    public static void RejectLinks(string path)
    {
        string? current = Path.GetFullPath(path);
        while (current is not null)
        {
            if (
                (File.Exists(current) || Directory.Exists(current))
                && (File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0
            )
            {
                throw LinkNotSupported();
            }
            current = Path.GetDirectoryName(current);
        }
    }

    /// <summary>Single definition of the error raised for any link or junction.</summary>
    public static SermofurException LinkNotSupported() =>
        new("unsafe_path", "Links and junctions are not supported in this version.", 4);

    public static bool Contains(string parent, string child)
    {
        string trimmedParent = Path.TrimEndingDirectorySeparator(parent);
        string trimmedChild = Path.TrimEndingDirectorySeparator(child);
        if (string.Equals(trimmedParent, trimmedChild, Comparison))
        {
            return true;
        }
        // A volume root (C:\ or /) keeps its trailing separator: do not double it.
        string prefix = Path.EndsInDirectorySeparator(trimmedParent)
            ? trimmedParent
            : trimmedParent + Path.DirectorySeparatorChar;
        return trimmedChild.StartsWith(prefix, Comparison);
    }

    /// <summary>
    /// Lexical normalization of a mapping: relative, without drive, contained in the root.
    /// Accepts both <c>/</c> and <c>\</c> as separators, so that a mapping typed on Windows, or a
    /// stored row holding backslashes, reads the same on every operating system (ADR 0009).
    /// Does not require the directory to exist and does not check links.
    /// </summary>
    public static string NormalizeMapping(string root, string relative)
    {
        string portable = relative.Replace('\\', MappingSeparator);
        if (Path.IsPathRooted(portable) || portable.Contains(':'))
        {
            throw new SermofurException("unsafe_path", "Relative mapping required.", 4);
        }
        string full = Path.TrimEndingDirectorySeparator(
            Path.GetFullPath(Path.Combine(root, portable))
        );
        if (!Contains(root, full))
        {
            throw new SermofurException("scope_boundary", "Mapping outside the instance.", 4);
        }
        return full;
    }

    /// <summary>Normalized mapping then checked physically: existing local directory, no link.</summary>
    public static string ResolveMapping(string root, string relative) =>
        ValidateDirectory(NormalizeMapping(root, relative));

    /// <summary>
    /// Mapping of <paramref name="absolute"/> relative to <paramref name="root"/>, with
    /// <see cref="MappingSeparator"/> as separator so that the stored form is the same on every
    /// operating system.
    /// </summary>
    public static string RelativizeMapping(string root, string absolute) =>
        Path.GetRelativePath(root, absolute).Replace(Path.DirectorySeparatorChar, MappingSeparator);

    /// <summary>
    /// Relative path (stored form) with each existing entry named as it is on disk. Only an entry
    /// that the file system opens under the given name but lists under another one is renamed:
    /// that is the same file on a case-insensitive (or normalization-insensitive) file system,
    /// Windows and macOS by default. On a case-sensitive file system, or for a missing entry, the
    /// given name is kept. Case and Unicode normalization (NFC/NFD) both come from the disk.
    /// </summary>
    /// <exception cref="SermofurException"><c>unsafe_path</c> for an alias or an ambiguous name.</exception>
    public static string CanonicalCase(string root, string relative)
    {
        string current = root;
        List<string> parts = new List<string>();
        foreach (
            string part in relative.Split(MappingSeparator, StringSplitOptions.RemoveEmptyEntries)
        )
        {
            string candidate = Path.Combine(current, part);
            string name = part;
            if (part != "." && (File.Exists(candidate) || Directory.Exists(candidate)))
            {
                // A folder that can be crossed but not listed keeps the given name.
                string[]? listed = Listed(current, part);
                if (listed is not null && !listed.Contains(part, StringComparer.Ordinal))
                {
                    // An entry opened under a name that no listed name reflects (a Windows 8.3
                    // short name) or that several names reflect cannot be given one stored path.
                    name =
                        listed.Length == 1
                            ? listed[0]
                            : throw new SermofurException(
                                "unsafe_path",
                                "Path reached through an alias (short name) or an ambiguous name.",
                                4
                            );
                }
            }
            parts.Add(name);
            current = Path.Combine(current, name);
        }
        return string.Join(MappingSeparator, parts);
    }

    /// <summary>Entries of <paramref name="directory"/> equal to <paramref name="name"/> without case and normalization.</summary>
    /// <returns>Null when the directory cannot be listed.</returns>
    private static string[]? Listed(string directory, string name)
    {
        string folded = name.Normalize(NormalizationForm.FormC);
        // The pattern only filters: the whole directory is read either way. * and ? are
        // wildcards, and a non-ASCII name may be stored under another normalization, so those
        // names are compared one by one.
        string pattern = name.Any(c => c is '*' or '?' || c > 127) ? "*" : name;
        try
        {
            return Matches(directory, folded, pattern);
        }
        catch (Exception exception) when (exception is UnauthorizedAccessException or IOException)
        {
            return null;
        }
    }

    private static string[] Matches(string directory, string folded, string pattern) =>
        new DirectoryInfo(directory)
            .EnumerateFileSystemInfos(
                pattern,
                new EnumerationOptions
                {
                    MatchCasing = MatchCasing.CaseInsensitive,
                    MatchType = MatchType.Simple,
                    IgnoreInaccessible = false,
                    AttributesToSkip = 0,
                }
            )
            .Select(entry => entry.Name)
            .Where(found =>
                string.Equals(
                    found.Normalize(NormalizationForm.FormC),
                    folded,
                    StringComparison.OrdinalIgnoreCase
                )
            )
            .ToArray();

    private static bool IsNetworkDrive(string full)
    {
        if (!OperatingSystem.IsWindows())
        {
            return false;
        }
        string? volume = Path.GetPathRoot(full);
        if (string.IsNullOrEmpty(volume))
        {
            return false;
        }
        return new DriveInfo(volume).DriveType == DriveType.Network;
    }
}
