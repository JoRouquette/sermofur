using System.Diagnostics;

namespace Sermofur.Daemon;

/// <summary>
/// One daemon per user (FR-018): an exclusive lock held for the life of the process. The lock is
/// released only once the daemon has drained its commands and exited, so a stopper that waits
/// for it knows the started commands have answered.
/// </summary>
public static class DaemonLock
{
    /// <summary>Takes the lock, or throws <c>daemon_already_running</c>.</summary>
    public static FileStream Acquire(DaemonPaths paths)
    {
        FileTurn.CreatePrivateFolder(paths.StateDirectory);
        try
        {
            return FileTurn.Take(paths.LockFile);
        }
        catch (IOException)
        {
            throw IpcEndpoint.AlreadyRunning();
        }
    }

    /// <summary>True while a daemon process holds the lock.</summary>
    public static bool IsHeld(string lockFile)
    {
        if (!File.Exists(lockFile))
        {
            return false;
        }
        try
        {
            using FileStream probe = new FileStream(
                lockFile,
                FileMode.Open,
                FileAccess.ReadWrite,
                FileShare.None
            );
            return false;
        }
        catch (FileNotFoundException)
        {
            return false;
        }
        catch (IOException)
        {
            return true;
        }
        catch (UnauthorizedAccessException)
        {
            // Not ours to probe: nothing to wait for.
            return false;
        }
    }

    /// <summary>Waits until no process holds the lock, or the timeout passes; true when released.</summary>
    public static async Task<bool> WaitForReleaseAsync(string lockFile, TimeSpan timeout)
    {
        Stopwatch waited = Stopwatch.StartNew();
        while (IsHeld(lockFile))
        {
            if (waited.Elapsed >= timeout)
            {
                return false;
            }
            await Task.Delay(100);
        }
        return true;
    }
}
