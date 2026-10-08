namespace Sermofur.Tests;

/// <summary>
/// A fact about Unix file ownership or special files: reported as skipped on Windows, with its
/// reason. On Linux and macOS it runs, and fails when its preconditions are missing.
/// </summary>
public sealed class UnixFactAttribute : FactAttribute
{
    public UnixFactAttribute()
    {
        if (OperatingSystem.IsWindows())
        {
            Skip = "Linux and macOS only: Unix owners and special files.";
        }
    }
}
