// Records what the real `smf` of the oracle (the .NET command line, already built) prints for
// the help of each command and each group (lot 9): every topic of CommandHelp.Topics with -h,
// every group (first word of a topic of two words) with -h, then edge cases of
// CommandHelp.Find and CommandArguments.Asks (help after --, unknown subcommand, options and
// their values, shortcuts). The topics and groups come from the published Sermofur.Cli.dll by
// reflection, not from a list written here. Each case runs in a fresh empty folder, with
// SERMOFUR_NO_DAEMON=1 and a daemon home and Codex home that do not exist; stdout and stderr
// are kept as printed (line breaks of the platform included), with the exit code.
//
// Usage: dotnet run help-oracle.cs -- OUTPUT.json [ORACLE]
// ORACLE: the oracle program; default SMF_ORACLE, then rust/target/oracle/Sermofur.Cli.exe
// (Windows) or ~/sermofur-oracle-bin/Sermofur.Cli (elsewhere). Sermofur.Cli.dll is read beside it.
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
using System.Collections;
using System.Diagnostics;
using System.Reflection;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;

string output = args[0];
string oracle =
    args.Length > 1 ? args[1]
    : Environment.GetEnvironmentVariable("SMF_ORACLE") is { Length: > 0 } variable ? variable
    : OperatingSystem.IsWindows()
        ? Path.GetFullPath(Path.Combine("..", "..", "target", "oracle", "Sermofur.Cli.exe"))
    : Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        "sermofur-oracle-bin",
        "Sermofur.Cli"
    );

Assembly cli = Assembly.LoadFrom(
    Path.Combine(Path.GetDirectoryName(Path.GetFullPath(oracle))!, "Sermofur.Cli.dll")
);
Type help = cli.GetType("Sermofur.Cli.CommandHelp", throwOnError: true)!;
List<string> commands = [];
foreach (object topic in (IEnumerable)help.GetProperty("Topics")!.GetValue(null)!)
{
    commands.Add((string)topic.GetType().GetProperty("Command")!.GetValue(topic)!);
}
List<string> groups = commands
    .Where(c => c.Contains(' '))
    .Select(c => c.Split(' ')[0])
    .Distinct()
    .ToList();

string elsewhere = Path.Combine(
    Path.GetTempPath(),
    "sermofur-help-oracle-nothing-" + Guid.NewGuid().ToString("N")
);
JsonArray cases = [];

void Case(string name, params string[] arguments)
{
    string folder = Path.Combine(
        Path.GetTempPath(),
        "sermofur-help-oracle-" + Guid.NewGuid().ToString("N")
    );
    Directory.CreateDirectory(folder);
    ProcessStartInfo start = new(oracle)
    {
        WorkingDirectory = folder,
        RedirectStandardOutput = true,
        RedirectStandardError = true,
        StandardOutputEncoding = new UTF8Encoding(false),
        StandardErrorEncoding = new UTF8Encoding(false),
    };
    foreach (string argument in arguments)
    {
        start.ArgumentList.Add(argument);
    }
    start.Environment["SERMOFUR_NO_DAEMON"] = "1";
    start.Environment["SERMOFUR_DAEMON_HOME"] = elsewhere;
    start.Environment["CODEX_HOME"] = elsewhere;
    using Process process = Process.Start(start)!;
    Task<string> error = process.StandardError.ReadToEndAsync();
    string text = process.StandardOutput.ReadToEnd();
    process.WaitForExit();
    if (Directory.EnumerateFileSystemEntries(folder).Any())
    {
        throw new InvalidOperationException($"{name}: the folder changed");
    }
    Directory.Delete(folder);
    JsonArray list = [];
    foreach (string argument in arguments)
    {
        list.Add(argument);
    }
    cases.Add(
        new JsonObject
        {
            ["name"] = name,
            ["arguments"] = list,
            ["exit_code"] = process.ExitCode,
            ["stdout"] = text,
            ["stderr"] = error.Result,
        }
    );
}

foreach (string command in commands)
{
    Case("topic " + command, [.. command.Split(' '), "-h"]);
}
foreach (string group in groups)
{
    Case("group " + group, group, "-h");
}

// Edge cases of CommandHelp.Find (positionals, then a topic of two words, of one, a group).
Case("long_form", "claim", "add", "--help");
Case("help_first", "-h", "claim", "add");
Case("help_between", "claim", "-h", "add");
Case("with_path", "--path", ".", "claim", "show", "-h");
Case("with_json", "--json", "claim", "add", "-h");
Case("json_between", "claim", "--json", "add", "-h");
Case("version_shortcut_skipped", "-v", "claim", "-h");
Case("help_wins_over_version", "recall", "--version", "--help");
Case("extra_positionals", "claim", "add", "x", "--help", "--json");
Case("option_with_value", "recall", "--limit", "2", "-h");
Case("all_arguments", "scope", "add", "a", "client", "workspace", "a", "-h");
Case("unknown_subcommand", "claim", "bogus", "-h");
Case("subcommand_of_another_group", "claim", "rebuild", "-h");
Case("extra_word_after_a_command", "init", "extra", "-h");
Case("extra_word_after_a_two_word_command", "daemon", "run", "now", "-h");
Case("group_name_alone_as_second_word", "status", "claim", "-h");
Case("unknown_command", "nothing", "-h");
Case("case_sensitive", "CLAIM", "-h");
Case("case_sensitive_subcommand", "claim", "ADD", "-h");
Case("path_value_skipped", "--path", ".", "-h");
Case("path_value_is_a_group", "--path", "claim", "-h");
Case("unknown_option_takes_the_group", "--x", "claim", "-h");
Case("unknown_option_takes_the_subcommand", "claim", "--x", "add", "-h");
Case("option_value_is_the_shortcut", "claim", "add", "--origin", "-h", "--help");
Case("flag_before_the_shortcut", "claim", "add", "--contradicts", "-h");
Case("option_before_double_dash_word", "claim", "add", "--origin", "--help");
Case("help_then_separator", "-h", "--", "claim");
Case("group_then_separator", "claim", "-h", "--", "add");
Case("separator_first", "--", "claim", "-h");
Case("help_after_separator", "status", "--", "-h");
Case("long_help_after_separator", "claim", "add", "--", "--help");
Case("group_without_subcommand", "claim");
Case("empty_word", "", "-h");
Case("empty_second_word", "claim", "", "-h");

File.WriteAllText(
    output,
    new JsonObject { ["cases"] = cases }
        .ToJsonString(
            new JsonSerializerOptions
            {
                WriteIndented = true,
                Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            }
        )
        .Replace("\r\n", "\n") + "\n"
);
Console.Error.WriteLine($"{cases.Count} cases ({commands.Count} topics, {groups.Count} groups)");
