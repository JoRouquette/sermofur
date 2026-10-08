namespace Sermofur.Tests;

/// <summary>
/// A reference measure, long to set up: it runs when <c>SERMOFUR_PERFORMANCE=1</c> and is
/// otherwise reported as skipped with that instruction, never passed silently.
/// </summary>
public sealed class PerformanceFactAttribute : FactAttribute
{
    public PerformanceFactAttribute()
    {
        if (Environment.GetEnvironmentVariable("SERMOFUR_PERFORMANCE") != "1")
        {
            Skip = "Performance measure: set SERMOFUR_PERFORMANCE=1 to run it.";
        }
    }
}
