using System.Text;
using System.Text.Json;
using Sermofur.Application;
using Sermofur.Cli;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class CliTests
{
    [Fact]
    public async Task RealProcessesExerciseInitRootStatusDoctorAndMemory()
    {
        using TestInstance fixture = new();
        CliResult init = await TestInstance.RunCli("--path", fixture.Root, "init", "--json");
        Assert.Equal(0, init.ExitCode);
        Assert.Empty(init.Error);
        Assert.False(
            JsonDocument.Parse(init.Output).RootElement.GetProperty("created").GetBoolean()
        );
        string nested = Path.Combine(fixture.Root, "src", "nested");
        Directory.CreateDirectory(nested);
        CliResult root = await TestInstance.RunCli("--path", nested, "root");
        Assert.Equal(fixture.Root, root.Output.Trim());
        CliResult created = await TestInstance.RunCli(
            "--path",
            nested,
            "claim",
            "add",
            "hypothèse CLI",
            "--origin",
            "user",
            "--key",
            "cli-claim",
            "--json"
        );
        Assert.Equal(0, created.ExitCode);
        MemoryRecord claim = RecordJson.Read<MemoryRecord>(created.Output);
        CliResult evidence = await TestInstance.RunCli(
            "--path",
            nested,
            "evidence",
            "add",
            claim.Id.ToString(),
            "source_code",
            "code://observé",
            "--lineage",
            "origin",
            "--origin",
            "user",
            "--json"
        );
        Assert.Equal(0, evidence.ExitCode);
        CliResult retex = await TestInstance.RunCli(
            "--path",
            nested,
            "retex",
            "add",
            "--event",
            "failure",
            "--impact",
            "wrong hypothesis",
            "--next",
            "measure",
            "--origin",
            "user",
            "--json"
        );
        Assert.Equal(0, retex.ExitCode);
        CliResult show = await TestInstance.RunCli(
            "--path",
            nested,
            "claim",
            "show",
            claim.Id.ToString(),
            "--json"
        );
        Assert.Equal(
            ConfidenceLevel.High,
            RecordJson.Read<ClaimExplanation>(show.Output).Confidence.Level
        );
        CliResult status = await TestInstance.RunCli("--path", nested, "status", "--json");
        Assert.Equal(
            1,
            JsonDocument.Parse(status.Output).RootElement.GetProperty("claims").GetInt32()
        );
        CliResult doctor = await TestInstance.RunCli("--path", nested, "doctor", "--json");
        Assert.Equal(0, doctor.ExitCode);
        Assert.Equal("healthy_with_warnings", RecordJson.Read<DoctorReport>(doctor.Output).Overall);
    }

    [Fact]
    public async Task CrossClientErrorDoesNotRevealContentAndStatusIsFiltered()
    {
        using TestInstance fixture = new();
        string b;
        Guid id;
        using (SqliteStore store = fixture.Open())
        {
            string a = fixture.Client(store, "a");
            b = fixture.Client(store, "b");
            id = fixture
                .Memory(store, a)
                .CreateClaim(TestInstance.Fact("secret client A"), TestInstance.User, null)
                .Id;
        }
        CliResult read = await TestInstance.RunCli(
            "--path",
            b,
            "claim",
            "show",
            id.ToString(),
            "--json"
        );
        Assert.Equal(1, read.ExitCode);
        Assert.Empty(read.Output);
        Assert.Equal("not_found", TestInstance.ErrorCode(read));
        Assert.DoesNotContain("secret", read.Error);
        CliResult status = await TestInstance.RunCli("--path", b, "status", "--json");
        Assert.Equal(
            0,
            JsonDocument.Parse(status.Output).RootElement.GetProperty("claims").GetInt32()
        );
        CliResult scopes = await TestInstance.RunCli("--path", b, "scope", "list", "--json");
        Assert.DoesNotContain("\"a\"", scopes.Output);
    }

    [Fact]
    public async Task RealProcessWritesUtf8StdoutWithNonAsciiTextAndUtf8StderrWithoutBom()
    {
        const string Text = "œuvre 漢 hypothèse";
        using TestInstance fixture = new();
        CliResult created = await TestInstance.RunCli(
            "--path",
            fixture.Root,
            "claim",
            "add",
            Text,
            "--origin",
            "user",
            "--json"
        );
        Assert.Equal(0, created.ExitCode);
        Guid id = RecordJson.Read<MemoryRecord>(created.Output).Id;
        CliResult shown = await TestInstance.RunCli(
            "--path",
            fixture.Root,
            "claim",
            "show",
            id.ToString(),
            "--json"
        );
        Assert.Equal(0, shown.ExitCode);
        // On the raw decoded output: a \u escape would not contain these characters.
        Assert.Contains("漢", shown.Output);
        Assert.Contains("hypothèse", shown.Output);
        ClaimExplanation explanation = RecordJson.Read<ClaimExplanation>(shown.Output);
        Assert.Equal(Text, RecordJson.Read<ClaimContent>(explanation.Claim.ContentJson).Text);
        RawCliResult rejected = await TestInstance.RunCliRaw(
            "--path",
            fixture.Root,
            "claim",
            "add",
            Text,
            "--origin",
            "user",
            "--actor",
            " ",
            "--json"
        );
        Assert.Equal(1, rejected.ExitCode);
        // Error messages are ASCII: what stays observable is the absence of BOM and strict UTF-8.
        Assert.NotEmpty(rejected.Error);
        Assert.Equal((byte)'{', rejected.Error[0]);
        string error = new UTF8Encoding(false, throwOnInvalidBytes: true).GetString(rejected.Error);
        // Literal string: pins the contract message, thousands separator included.
        Assert.Equal(
            "Text required, limited to 16,384 characters without NUL.",
            JsonDocument.Parse(error).RootElement.GetProperty("message").GetString()
        );
    }

    [Fact]
    public void ArgumentCountIsAcceptedAtTheLimitAndRejectedBeyond()
    {
        using TestInstance fixture = new();
        Dictionary<string, string> before = fixture.Snapshot();
        // init refuses any extra positional before writing: only the reason for refusal matters.
        CliResult atLimit = TestInstance.Run(
            InitWithArgumentCount(fixture, InputLimits.MaxArguments)
        );
        CliResult beyond = TestInstance.Run(
            InitWithArgumentCount(fixture, InputLimits.MaxArguments + 1)
        );
        Assert.Equal(1, atLimit.ExitCode);
        Assert.Empty(atLimit.Output);
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(atLimit));
        Assert.Equal("Wrong number of arguments.", ErrorMessage(atLimit));
        Assert.Equal(1, beyond.ExitCode);
        Assert.Empty(beyond.Output);
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(beyond));
        Assert.Equal(InputLimits.ArgumentCountMessage, ErrorMessage(beyond));
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void ArgumentLengthIsRejectedByTheCliBeforeTheScopeService()
    {
        using TestInstance fixture = new();
        Dictionary<string, string> before = fixture.Snapshot();
        string overlong = new('a', InputLimits.MaxTextLength + 1);
        CliResult result = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "scope",
            "add",
            "x",
            "client",
            ScopePolicy.WorkspaceScopeId,
            overlong
        );
        Assert.Equal(1, result.ExitCode);
        Assert.Empty(result.Output);
        // invalid_scope would signal a refusal by the service: the CLI bound comes first.
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(result));
        Assert.Equal("Argument limited to 16,384 characters without NUL.", ErrorMessage(result));
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void ArgumentContainingNulIsRejectedBeforeAnyCommand()
    {
        using TestInstance fixture = new();
        Dictionary<string, string> before = fixture.Snapshot();
        CliResult result = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "claim",
            "add",
            "before\0after",
            "--origin",
            "user"
        );
        Assert.Equal(1, result.ExitCode);
        Assert.Empty(result.Output);
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(result));
        Assert.Equal(InputLimits.ArgumentMessage, ErrorMessage(result));
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    private static string[] InitWithArgumentCount(TestInstance fixture, int total)
    {
        string[] prefix = ["--path", fixture.Root, "--json", "init"];
        return prefix.Concat(Enumerable.Repeat("x", total - prefix.Length)).ToArray();
    }

    private static string? ErrorMessage(CliResult result) =>
        JsonDocument.Parse(result.Error).RootElement.GetProperty("message").GetString();

    [Fact]
    public void UsageGivesOneLinePerFormWithoutMixingOptionValuesAndSubcommands()
    {
        CliResult result = TestInstance.Run("--help");
        Assert.Equal(0, result.ExitCode);
        Assert.Contains("smf claim list | show ID | invalidate ID --reason TEXT", result.Output);
        Assert.Contains("smf evidence list | show ID", result.Output);
        Assert.Contains("smf retex list | show ID", result.Output);
        Assert.Contains("--help is recognized anywhere before --", result.Output);
        Assert.DoesNotContain("llm|list", result.Output);
        Assert.DoesNotContain("tree|add", result.Output);
    }

    [Fact]
    public void UsageShowsTheBuildVersionAndTheMatchingCliReference()
    {
        CliResult result = TestInstance.Run("--help");
        string version = ProductVersion.Current;
        // Anchor independent of the code under test: the assembly version comes from the same
        // MSBuild Version property (0.0.0 for 0.0.0-dev, 9.9.9 for -p:Version=9.9.9).
        Assert.Equal(
            typeof(ProductVersion).Assembly.GetName().Version!.ToString(3),
            version.Split('-')[0]
        );
        Assert.StartsWith($"Sermofur {version}", result.Output);
        Assert.Contains(ProductVersion.CliDocumentation(version), result.Output);
    }

    [Theory]
    [InlineData(null, "0.0.0-dev")]
    [InlineData("", "0.0.0-dev")]
    [InlineData("1.2.3", "1.2.3")]
    [InlineData("1.2.3+abc123", "1.2.3")]
    [InlineData("0.0.0-dev+abc123", "0.0.0-dev")]
    public void InformationalVersionLosesItsBuildMetadata(string? informational, string expected)
    {
        Assert.Equal(expected, ProductVersion.Normalize(informational));
    }

    [Theory]
    [InlineData("0.0.0-dev", "https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md")]
    [InlineData("1.2.3", "https://github.com/JoRouquette/sermofur/blob/v1.2.3/docs/cli.md")]
    public void CliReferencePointsToTheReleaseTagOrToMainForADevelopmentBuild(
        string version,
        string expected
    )
    {
        Assert.Equal(expected, ProductVersion.CliDocumentation(version));
    }

    [Theory]
    [InlineData("claim", "add", "x", "--origin", "user", "--scope", "someone")]
    [InlineData("status", "--unknown", "value")]
    [InlineData("claim", "show", "not-an-id")]
    [InlineData("scope", "add", "x", "99", "workspace", "x")]
    [InlineData("scope", "add", "Bad", "client", "workspace", "x")]
    [InlineData("claim", "add", "x")]
    [InlineData("claim", "add", "--", "x", "--origin", "user")]
    [InlineData("claim", "add", "x", "--origin", "machine")]
    public async Task InvalidArgumentsAreRejectedWithoutMutation(params string[] arguments)
    {
        using TestInstance fixture = new();
        Dictionary<string, string> before = fixture.Snapshot();
        CliResult result = await TestInstance.RunCli(
            new[] { "--path", fixture.Root, "--json" }.Concat(arguments).ToArray()
        );
        Assert.Equal(1, result.ExitCode);
        Assert.Empty(result.Output);
        JsonDocument.Parse(result.Error);
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Theory]
    [InlineData("--help")]
    [InlineData("status", "--help")]
    [InlineData("claim", "add", "x", "--help", "--json")]
    [InlineData("--json", "--help", "--", "init")]
    public void HelpAnywhereShowsUsageBeforeInstanceResolution(params string[] arguments)
    {
        // Directory without an instance: without help taking priority, the command would exit 2.
        string outside = Path.Combine(
            TestInstance.TempRoot,
            "sermofur-help-" + Guid.NewGuid().ToString("N")
        );
        Directory.CreateDirectory(outside);
        try
        {
            CliResult result = TestInstance.Run(
                new[] { "--path", outside }.Concat(arguments).ToArray()
            );
            Assert.Equal(0, result.ExitCode);
            Assert.StartsWith($"Sermofur {ProductVersion.Current}", result.Output);
            Assert.Empty(result.Error);
            Assert.False(Directory.Exists(Path.Combine(outside, ".sermofur")));
        }
        finally
        {
            Directory.Delete(outside, true);
        }
    }

    [Fact]
    public void DoubleDashMakesFollowingArgumentsPositional()
    {
        using TestInstance fixture = new();
        CliResult created = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "claim",
            "add",
            "--origin",
            "user",
            "--",
            "--text starting with two dashes"
        );
        Assert.Equal(0, created.ExitCode);
        Assert.Equal(
            "--text starting with two dashes",
            RecordJson
                .Read<ClaimContent>(RecordJson.Read<MemoryRecord>(created.Output).ContentJson)
                .Text
        );
        CliResult helpAsText = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "claim",
            "add",
            "--origin",
            "user",
            "--",
            "--help"
        );
        Assert.Equal(0, helpAsText.ExitCode);
        Assert.DoesNotContain("Global options", helpAsText.Output);
    }

    [Fact]
    public void MissingOriginIsRejectedEvenForDecisionsAndWritesNothing()
    {
        using TestInstance fixture = new();
        Dictionary<string, string> before = fixture.Snapshot();
        CliResult result = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "claim",
            "add",
            "always review before commit",
            "--category",
            "decisions"
        );
        Assert.Equal(1, result.ExitCode);
        Assert.Empty(result.Output);
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(result));
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void AccentsStayLiteralWhileBackticksCannotCloseTheJsonBlock()
    {
        using TestInstance fixture = new();
        CliResult created = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "claim",
            "add",
            "hypothèse ``` <b>fin</b>",
            "--origin",
            "user"
        );
        Assert.Equal(0, created.ExitCode);
        Assert.Contains("hypothèse", created.Output);
        MemoryRecord record = RecordJson.Read<MemoryRecord>(created.Output);
        string projection = File.ReadAllText(new MarkdownProjection(fixture.Root).PathFor(record));
        Assert.Contains("hypothèse", projection);
        Assert.DoesNotContain("<b>", projection);
        string[] fences = projection.Split("```");
        Assert.Equal(3, fences.Length);
        Assert.Equal(
            "hypothèse ``` <b>fin</b>",
            RecordJson.Read<ClaimContent>(record.ContentJson).Text
        );
    }

    [Fact]
    public async Task ConcurrentFirstInitPublishesOnlyOneCompleteIdentity()
    {
        TestInstance.RequireNoEntryAboveTemp();
        string root = Path.Combine(
            TestInstance.TempRoot,
            "sermofur-init-test-" + Guid.NewGuid().ToString("N")
        );
        Directory.CreateDirectory(root);
        try
        {
            CliResult[] results = await Task.WhenAll(
                Enumerable
                    .Range(0, 4)
                    .Select(_ => TestInstance.RunCli("--path", root, "init", "--json"))
            );
            Assert.All(results, result => Assert.Equal(0, result.ExitCode));
            Assert.Single(
                results
                    .Select(result =>
                        JsonDocument
                            .Parse(result.Output)
                            .RootElement.GetProperty("configuration")
                            .GetProperty("instanceId")
                            .GetString()
                    )
                    .Distinct()
            );
            Assert.Equal(
                1,
                results.Count(result =>
                    JsonDocument
                        .Parse(result.Output)
                        .RootElement.GetProperty("created")
                        .GetBoolean()
                )
            );
            Assert.Single(Directory.GetDirectories(root));
            Assert.Equal("healthy_with_warnings", new InstanceDoctor().Inspect(root).Overall);
        }
        finally
        {
            Directory.Delete(root, true);
        }
    }
}
