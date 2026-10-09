using System.Diagnostics;

namespace Sermofur.Daemon;

/// <summary>
/// A lock file beside shared state, held while one process changes that state: exclusive across
/// processes (share mode on Windows, <c>flock</c> on Unix), private to its owner, never written,
/// never deleted (deleting it would let two processes lock two different files).
/// </summary>
internal static class FileTurn
{
    private const UnixFileMode PrivateFile = UnixFileMode.UserRead | UnixFileMode.UserWrite;
    private const UnixFileMode PrivateFolder = PrivateFile | UnixFileMode.UserExecute;

    /// <summary>
    /// Opens <paramref name="lockFile"/> exclusively; an <see cref="IOException"/> when another
    /// process holds it. The lock file is readable by its owner only, so that no other account
    /// can lock it: a new one is created so, and one left by an older version with wider rights
    /// is narrowed.
    /// </summary>
    public static FileStream Take(string lockFile)
    {
        FileStreamOptions options = new FileStreamOptions
        {
            Mode = FileMode.OpenOrCreate,
            Access = FileAccess.ReadWrite,
            Share = FileShare.None,
        };
        if (OperatingSystem.IsWindows())
        {
            return new FileStream(lockFile, options);
        }
        options.UnixCreateMode = PrivateFile;
        FileStream stream = new FileStream(lockFile, options);
        try
        {
            if (File.GetUnixFileMode(stream.SafeFileHandle) != PrivateFile)
            {
                File.SetUnixFileMode(stream.SafeFileHandle, PrivateFile);
            }
        }
        catch (UnauthorizedAccessException)
        {
            // Not ours to change: the lock still works for this process.
        }
        return stream;
    }

    /// <summary>
    /// Waits for the turn up to <paramref name="timeout"/>, trying again every
    /// <paramref name="poll"/>; null when it did not come.
    /// </summary>
    public static FileStream? TryTake(string lockFile, TimeSpan timeout, TimeSpan poll)
    {
        CreatePrivateFolder(Path.GetDirectoryName(Path.GetFullPath(lockFile))!);
        Stopwatch waited = Stopwatch.StartNew();
        while (true)
        {
            try
            {
                return Take(lockFile);
            }
            catch (IOException) when (waited.Elapsed < timeout)
            {
                Thread.Sleep(poll);
            }
            catch (IOException)
            {
                return null;
            }
        }
    }

    /// <summary>
    /// Creates a missing folder, open to its owner only on Unix. An existing folder keeps its
    /// mode: the user may have set it on purpose.
    /// </summary>
    public static void CreatePrivateFolder(string folder)
    {
        if (Directory.Exists(folder))
        {
            return;
        }
        if (OperatingSystem.IsWindows())
        {
            Directory.CreateDirectory(folder);
        }
        else
        {
            Directory.CreateDirectory(folder, PrivateFolder);
        }
    }
}
