namespace Sermofur.Tests;

/// <summary>
/// A fact that runs on Windows only and is reported as skipped elsewhere, with its reason,
/// instead of passing silently.
/// </summary>
public sealed class WindowsFactAttribute : FactAttribute
{
    public WindowsFactAttribute(string reason)
    {
        if (!OperatingSystem.IsWindows())
        {
            Skip = $"Windows only: {reason}";
        }
    }
}
