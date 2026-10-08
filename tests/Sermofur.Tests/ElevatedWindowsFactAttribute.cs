using System.Security.Principal;

namespace Sermofur.Tests;

/// <summary>
/// A fact that changes file owners on Windows: it needs an elevated session, as on the CI runner.
/// Reported as skipped elsewhere or without elevation, with that reason.
/// </summary>
public sealed class ElevatedWindowsFactAttribute : FactAttribute
{
    public ElevatedWindowsFactAttribute()
    {
        if (!OperatingSystem.IsWindows())
        {
            Skip = "Windows only: owners are SIDs.";
            return;
        }
        using WindowsIdentity identity = WindowsIdentity.GetCurrent();
        if (!new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator))
        {
            Skip = "Needs an elevated session to change the owner of a folder.";
        }
    }
}
