namespace Sermofur.Tests;

/// <summary>
/// A fact about a symbolic link to a file. Unix always allows one; Windows needs an elevated
/// session or developer mode, so the ability is probed once and the fact is reported as skipped
/// with that reason when no link can be made, like the facts about owners.
/// </summary>
public sealed class FileLinkFactAttribute : FactAttribute
{
    private static readonly string? Refusal = Probe();

    public FileLinkFactAttribute() => Skip = Refusal;

    private static string? Probe()
    {
        if (!OperatingSystem.IsWindows())
        {
            return null;
        }
        // The resolved temporary root the whole suite uses: on macOS the temporary folder
        // lives under /var, itself a link, which is exactly what a probe about links must not
        // walk into.
        string folder = Path.Combine(
            TestInstance.TempRoot,
            "smf-link-" + Guid.NewGuid().ToString("N")[..8]
        );
        try
        {
            Directory.CreateDirectory(folder);
            string target = Path.Combine(folder, "target");
            File.WriteAllText(target, "x");
            File.CreateSymbolicLink(Path.Combine(folder, "link"), target);
            return null;
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            return "Needs an elevated session or developer mode to link to a file.";
        }
        finally
        {
            try
            {
                Directory.Delete(folder, true);
            }
            catch (Exception exception)
                when (exception is IOException or UnauthorizedAccessException)
            {
                // A leftover in the temporary folder is not worth failing a probe.
            }
        }
    }
}
