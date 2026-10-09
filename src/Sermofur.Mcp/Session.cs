namespace Sermofur.Mcp;

/// <summary>Folder and identity of an MCP session (research R3, R4).</summary>
public static class Session
{
    /// <summary>Variable Claude Code sets to the project root in the server environment.</summary>
    public const string ProjectDirectoryVariable = "CLAUDE_PROJECT_DIR";

    public const string DefaultActor = "mcp-client";

    public const int MaxActorLength = 64;

    /// <summary>
    /// Launch folder of the bridge: <c>CLAUDE_PROJECT_DIR</c> when set and absolute, the current
    /// directory otherwise. The scope of the whole session comes from it.
    /// </summary>
    public static string LaunchFolder(Func<string, string?> variable, string currentDirectory)
    {
        string? project = variable(ProjectDirectoryVariable);
        string folder =
            !string.IsNullOrWhiteSpace(project) && Path.IsPathFullyQualified(project)
                ? project
                : currentDirectory;
        return Path.TrimEndingDirectorySeparator(Sermofur.Daemon.LongPath.Of(folder));
    }

    /// <summary>
    /// Actor of the writes of a session, from the name the host gives at initialization:
    /// lower case, <c>[a-z0-9-]</c>, at most 64 characters; <c>mcp-client</c> when nothing is left.
    /// </summary>
    public static string Actor(string? clientName)
    {
        if (string.IsNullOrWhiteSpace(clientName))
        {
            return DefaultActor;
        }
        System.Text.StringBuilder actor = new System.Text.StringBuilder();
        bool dash = false;
        foreach (char character in clientName.Trim().ToLowerInvariant())
        {
            if (character is (>= 'a' and <= 'z') or (>= '0' and <= '9'))
            {
                actor.Append(character);
                dash = false;
            }
            else if (!dash && actor.Length > 0)
            {
                actor.Append('-');
                dash = true;
            }
            if (actor.Length >= MaxActorLength)
            {
                break;
            }
        }
        string normalized = actor.ToString().Trim('-');
        return normalized.Length == 0 ? DefaultActor : normalized;
    }
}
