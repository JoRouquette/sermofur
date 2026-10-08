namespace Sermofur.Tests;

/// <summary>
/// A fact that installs a real service in the session of the current user: it runs in CI
/// (<c>GITHUB_ACTIONS=true</c>) or when <c>SERMOFUR_SERVICE_TESTS=1</c>, never by surprise on a
/// developer machine. Where it runs, a missing service manager fails it instead of skipping it.
/// </summary>
public sealed class ServiceFactAttribute : FactAttribute
{
    public ServiceFactAttribute()
    {
        if (
            Environment.GetEnvironmentVariable("GITHUB_ACTIONS") != "true"
            && Environment.GetEnvironmentVariable("SERMOFUR_SERVICE_TESTS") != "1"
        )
        {
            Skip = "Installs a real service: runs in CI, or set SERMOFUR_SERVICE_TESTS=1.";
        }
    }
}
