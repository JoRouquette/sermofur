namespace Sermofur.Daemon;

/// <summary>
/// Replaces a configuration file in one step: a temporary file of a random name, created new in
/// the same folder (never through an existing link), then renamed over the target. On Unix the
/// target keeps its permissions; a new file gets <c>newFileMode</c> when given.
/// </summary>
public static class AtomicFile
{
    public static void Write(string file, byte[] content, UnixFileMode? newFileMode = null)
    {
        string folder = Path.GetDirectoryName(Path.GetFullPath(file))!;
        Directory.CreateDirectory(folder);
        UnixFileMode? mode = null;
        if (!OperatingSystem.IsWindows())
        {
            mode = File.Exists(file) ? File.GetUnixFileMode(file) : newFileMode;
        }
        string temporary = Path.Combine(
            folder,
            $".{Path.GetFileName(file)}.{Path.GetRandomFileName()}.tmp"
        );
        try
        {
            FileStreamOptions options = new FileStreamOptions
            {
                Mode = FileMode.CreateNew,
                Access = FileAccess.Write,
                Share = FileShare.None,
            };
            if (mode is UnixFileMode born && !OperatingSystem.IsWindows())
            {
                // Born with the final mode (the umask can only narrow it): a private content is
                // never readable by others, not even before the rename.
                options.UnixCreateMode = born;
            }
            using (FileStream stream = new FileStream(temporary, options))
            {
                stream.Write(content);
                stream.Flush(true);
            }
            if (mode is UnixFileMode kept && !OperatingSystem.IsWindows())
            {
                // Undoes the umask, so the file ends with exactly the mode it had.
                File.SetUnixFileMode(temporary, kept);
            }
            File.Move(temporary, file, overwrite: true);
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
        }
    }
}
