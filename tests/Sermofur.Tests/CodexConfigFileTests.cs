using System.Text.Json;
using FsCheck;
using FsCheck.Fluent;
using FsCheck.Xunit;
using Sermofur.Domain;
using Sermofur.Mcp;

namespace Sermofur.Tests;

public class CodexConfigFileTests
{
    private static readonly string[] Serve = ["mcp", "serve"];

    private const string Existing = """
        # personal Codex configuration
        model = "gpt-5"   # comment kept

        [profiles.work]
        approval_policy = "on-request"

        [mcp_servers.github]
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-github"]
        notes = '''
        [mcp_servers.sermofur]
        not a header, inside a multi-line string
        '''

        [mcp_servers.github.env]
        TOKEN = "x"

        [[hooks]]
        name = "a"
        """;

    [Fact]
    public void AbsentFileGetsOnlyTheBlock()
    {
        string created = CodexConfigFile.WithBlock(null, "smf", Serve)!;
        Assert.Equal("[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\n", created);
        Assert.True(CodexConfigFile.Declares(created));
        Assert.Null(CodexConfigFile.WithBlock(created, "smf", Serve));
        Assert.Equal("", CodexConfigFile.WithoutBlock(created));
    }

    [Fact]
    public void EverythingElseStaysByteForByte()
    {
        string original = Existing + "\n";
        Assert.False(CodexConfigFile.Declares(original));
        string installed = CodexConfigFile.WithBlock(original, "smf", Serve)!;
        Assert.StartsWith(original, installed);
        Assert.Null(CodexConfigFile.WithBlock(installed, "smf", Serve));
        string updated = CodexConfigFile.WithBlock(
            installed,
            @"C:\Users\me\.dotnet\tools\smf.exe",
            Serve
        )!;
        Assert.Contains(@"command = 'C:\Users\me\.dotnet\tools\smf.exe'", updated);
        Assert.StartsWith(original, updated);
        Assert.Equal(original, CodexConfigFile.WithoutBlock(updated));
    }

    [Fact]
    public void BlockInTheMiddleIsReplacedAndRemovedWithItsSubTables()
    {
        string content =
            "[a]\nx = 1\n\n[mcp_servers.sermofur]\ncommand = 'old'\n\n[mcp_servers.sermofur.env]\nK = 'v'\n\n[b]\ny = 2\n";
        string updated = CodexConfigFile.WithBlock(content, "smf", Serve)!;
        // Only command and args are ours: the env sub-table the user added stays.
        Assert.Equal(
            "[a]\nx = 1\n\n[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\n\n[mcp_servers.sermofur.env]\nK = 'v'\n\n[b]\ny = 2\n",
            updated
        );
        string withApproval = updated.Replace(
            "args = ['mcp', 'serve']\n",
            "args = ['mcp', 'serve']\ndefault_tools_approval_mode = \"writes\"\n"
        );
        Assert.Null(CodexConfigFile.WithBlock(withApproval, "smf", Serve));
        Assert.Contains(
            "default_tools_approval_mode = \"writes\"",
            CodexConfigFile.WithBlock(withApproval, "/opt/smf", Serve)
        );
        Assert.Equal(
            "invalid_mcp_config",
            Assert
                .Throws<SermofurException>(() =>
                    CodexConfigFile.WithBlock(
                        "[mcp_servers.sermofur]\nargs = [\n  'x',\n]\n",
                        "smf",
                        Serve
                    )
                )
                .Code
        );
        Assert.Equal("[a]\nx = 1\n\n[b]\ny = 2\n", CodexConfigFile.WithoutBlock(updated));
    }

    [Fact]
    public void CrlfFilesStayCrlfAndQuotesAreEscaped()
    {
        string content = "model = \"x\"\r\n";
        string installed = CodexConfigFile.WithBlock(content, "C:\\it's\\smf.exe", Serve)!;
        Assert.Equal(
            "model = \"x\"\r\n\r\n[mcp_servers.sermofur]\r\ncommand = \"C:\\\\it's\\\\smf.exe\"\r\nargs = ['mcp', 'serve']\r\n",
            installed
        );
        Assert.Equal(content, CodexConfigFile.WithoutBlock(installed));
    }

    [Theory]
    [InlineData("[mcp_servers]\nsermofur = { command = \"smf\" }\n")]
    [InlineData("mcp_servers.sermofur.command = \"smf\"\n")]
    [InlineData("mcp_servers = { sermofur = { command = \"smf\" } }\n")]
    [InlineData("notes = \"\"\"\nnever closed\n")]
    [InlineData("[mcp_servers.sermofur]\ncommand = 'a'\n[mcp_servers.sermofur]\ncommand = 'b'\n")]
    public void UnsafeFilesAreRefused(string content)
    {
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            CodexConfigFile.WithBlock(content, "smf", Serve)
        );
        Assert.Equal(("invalid_mcp_config", 3), (refusal.Code, refusal.ExitCode));
    }

    [Fact]
    public void ArrayValuesAreNotTableHeaders()
    {
        string content = "matrix = [\n  [\"a\", \"b\"],\n  [\"c\"]\n]\n";
        string installed = CodexConfigFile.WithBlock(content, "smf", Serve)!;
        Assert.Equal(content, CodexConfigFile.WithoutBlock(installed));
    }

    [Theory]
    [InlineData("\n")]
    [InlineData("\r\n")]
    public void CommentsAboveTheNextTableStayOnRemoval(string newline)
    {
        string before = $"[a]{newline}x = 1{newline}{newline}";
        string after =
            $"# docs server{newline}# second line{newline}[mcp_servers.docs]{newline}command = 'd'{newline}";
        string block =
            $"[mcp_servers.sermofur]{newline}# ours{newline}command = 'smf'{newline}args = ['mcp', 'serve']{newline}{newline}";
        Assert.Equal(before + after, CodexConfigFile.WithoutBlock(before + block + after));
        string updated = CodexConfigFile.WithBlock(before + block + after, "/opt/smf", Serve)!;
        Assert.EndsWith(after, updated);
        Assert.Contains("# ours", updated);
    }

    [Fact]
    public void MissingFinalNewlineIsTheOnlyByteAddedForGood()
    {
        string content = "a = 1";
        string installed = CodexConfigFile.WithBlock(content, "smf", Serve)!;
        // Documented exception: the line break the header needs stays after removal.
        Assert.Equal("a = 1\n", CodexConfigFile.WithoutBlock(installed));
    }

    /// <summary>
    /// Whatever the other tables (with comments, sub-tables, CRLF, no final line break),
    /// installing then removing gives back the same text, plus a final line break if it lacked
    /// one.
    /// </summary>
    [Property(MaxTest = 300)]
    public Property InstallThenRemoveIsTheIdentity(
        NonEmptyString[] names,
        bool crlf,
        bool comments,
        bool finalNewline
    )
    {
        string newline = crlf ? "\r\n" : "\n";
        string content = Tables(Clean(names), newline, comments);
        if (!finalNewline && content.Length > 0)
        {
            content = content.TrimEnd('\r', '\n');
        }
        string expected =
            content.Length == 0 || content.EndsWith('\n') ? content : content + newline;
        string installed = CodexConfigFile.WithBlock(content, "smf", Serve)!;
        return (CodexConfigFile.WithoutBlock(installed) == expected).ToProperty();
    }

    /// <summary>A block between other tables goes away alone: what is around it stays.</summary>
    [Property(MaxTest = 300)]
    public Property RemovalFromTheMiddleKeepsTheRest(
        NonEmptyString[] first,
        NonEmptyString[] second,
        bool crlf
    )
    {
        string newline = crlf ? "\r\n" : "\n";
        string before = Tables(Clean(first), newline, true);
        // At least one table after the block: removal at the end of the file is the case above.
        string after = Tables([.. Clean(second), "last"], newline, true);
        string block =
            $"[mcp_servers.sermofur]{newline}command = 'smf'{newline}args = ['mcp', 'serve']{newline}{newline}[mcp_servers.sermofur.env]{newline}K = 'v'{newline}{newline}";
        return (
            CodexConfigFile.WithoutBlock(before + block + after) == before + after
        ).ToProperty();
    }

    private static string[] Clean(NonEmptyString[] names) =>
        [
            .. names
                .Select(name => new string([.. name.Get.Where(char.IsAsciiLetterOrDigit)]))
                .Where(name => name.Length > 0 && name != "sermofur")
                .Distinct(),
        ];

    private static string Tables(string[] names, string newline, bool comments) =>
        string.Concat(
            names.Select(name =>
                (comments ? $"# server {name}{newline}" : "")
                + $"[mcp_servers.{name}]{newline}command = '{name}'{newline}"
                + $"[mcp_servers.{name}.env]{newline}K = '{name}'  # kept{newline}{newline}"
            )
        );

    [Fact]
    public void CodexDeclarationThroughTheCli()
    {
        using TestInstance fixture = new TestInstance();
        CliResult installed = DaemonTests.Direct(
            fixture.Root,
            ["mcp", "install", "--host", "codex", "--json"]
        );
        Assert.Equal(0, installed.ExitCode);
        JsonElement report = JsonDocument.Parse(installed.Output).RootElement;
        Assert.Equal("codex", report.GetProperty("host").GetString());
        Assert.Contains(
            report.GetProperty("next").EnumerateArray(),
            step => step.GetString()!.Contains("trust")
        );
        Assert.True(File.Exists(Path.Combine(fixture.Root, ".codex", "config.toml")));
        CliResult doctor = DaemonTests.Direct(fixture.Root, ["doctor", "--json"]);
        Assert.Contains("codex (.codex/config.toml)", doctor.Output);
        Assert.Equal(
            0,
            DaemonTests.Direct(fixture.Root, ["mcp", "uninstall", "--host", "codex"]).ExitCode
        );
        Assert.Equal("", File.ReadAllText(Path.Combine(fixture.Root, ".codex", "config.toml")));
        CliResult user = DaemonTests.Direct(
            fixture.Root,
            ["mcp", "install", "--host", "codex", "--scope", "user", "--json"]
        );
        string userFile = JsonDocument
            .Parse(user.Output)
            .RootElement.GetProperty("file")
            .GetString()!;
        Assert.StartsWith(
            Environment.GetEnvironmentVariable(CodexConfigFile.HomeVariable)!,
            userFile
        );
        Assert.Equal(
            0,
            DaemonTests
                .Direct(fixture.Root, ["mcp", "uninstall", "--host", "codex", "--scope", "user"])
                .ExitCode
        );
        CliResult refused = DaemonTests.Direct(
            fixture.Root,
            ["mcp", "install", "--scope", "user", "--json"]
        );
        Assert.Equal(1, refused.ExitCode);
        Assert.Contains("claude mcp add --scope user", refused.Error);
        Assert.Equal(
            1,
            DaemonTests.Direct(fixture.Root, ["mcp", "install", "--host", "cursor"]).ExitCode
        );
    }
}
