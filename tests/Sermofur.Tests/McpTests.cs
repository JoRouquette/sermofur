using System.Diagnostics;
using System.Text.Json;
using ModelContextProtocol.Client;
using ModelContextProtocol.Protocol;
using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;
using Sermofur.Mcp;

namespace Sermofur.Tests;

/// <summary>The MCP bridge (spec 005): real protocol, tools, channel, errors.</summary>
[Collection("Daemon timing")]
public class McpTests
{
    private static readonly string[] ToolNames =
    [
        "sermofur_challenge",
        "sermofur_claim",
        "sermofur_context",
        "sermofur_evidence",
        "sermofur_feedback",
        "sermofur_recall",
        "sermofur_record_retex",
        "sermofur_status",
    ];

    [Fact]
    public async Task RealProtocolListsTheEightToolsAndRunsThem()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using McpClient client = await Client(fixture.Root, daemon.Home);
        IList<McpClientTool> tools = await client.ListToolsAsync();
        Assert.Equal(ToolNames, tools.Select(tool => tool.Name).Order(StringComparer.Ordinal));
        foreach (McpClientTool tool in tools)
        {
            JsonElement schema = tool.JsonSchema;
            Assert.Equal("object", schema.GetProperty("type").GetString());
            Assert.False(schema.GetProperty("additionalProperties").GetBoolean());
            Assert.False(tool.Description.Length == 0);
        }
        CallToolResult claim = await client.CallToolAsync(
            "sermofur_claim",
            new Dictionary<string, object?> { ["text"] = "Billing paginates with offsets" }
        );
        Assert.NotEqual(true, claim.IsError);
        CallToolResult recall = await client.CallToolAsync(
            "sermofur_recall",
            new Dictionary<string, object?> { ["question"] = "billing pagination" }
        );
        Assert.NotEqual(true, recall.IsError);
        string direct = DaemonTests
            .Direct(fixture.Root, ["recall", "--json", "--", "billing pagination"])
            .Output;
        Assert.Equal(Normalize(direct), Normalize(Text(recall)));
        JsonElement listed = JsonDocument
            .Parse(DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]).Output)
            .RootElement[0];
        Assert.Contains("\"llm\"", listed.GetRawText().ToLowerInvariant());
        Assert.Contains("claude-code-test", listed.GetRawText());
    }

    [Fact]
    public async Task WithoutDaemonTheBridgeAnswersWithTheRemedy()
    {
        using TestInstance fixture = new TestInstance();
        string home = TestDaemon.NewHome();
        try
        {
            await using McpClient client = await Client(fixture.Root, home);
            Assert.Equal(8, (await client.ListToolsAsync()).Count);
            Stopwatch watch = Stopwatch.StartNew();
            CallToolResult result = await client.CallToolAsync("sermofur_status");
            Assert.True(watch.Elapsed < TimeSpan.FromSeconds(1), $"{watch.ElapsedMilliseconds} ms");
            Assert.True(result.IsError);
            Assert.StartsWith("daemon_unavailable:", Text(result));
            Assert.Contains("smf daemon install", Text(result));
            CallToolResult again = await client.CallToolAsync("sermofur_status");
            Assert.True(again.IsError);
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public async Task ReadToolsGiveWhatTheCliGives()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        string claim = Id(
            DaemonTests
                .Direct(
                    fixture.Root,
                    ["claim", "add", "cache invalidation is hard", "--origin", "user", "--json"]
                )
                .Output
        );
        await using Bridge bridge = await Bridge.Open(daemon, fixture.Root);
        Assert.Equal(
            Normalize(
                DaemonTests
                    .Direct(fixture.Root, ["status", "--json"])
                    .Output.Replace("\"direct\"", "\"daemon\"")
            ),
            Normalize(await bridge.Ok("sermofur_status"))
        );
        Assert.Equal(
            Normalize(DaemonTests.Direct(fixture.Root, ["challenge", claim, "--json"]).Output),
            Normalize(await bridge.Ok("sermofur_challenge", ("claimId", claim)))
        );
        Assert.Equal(
            Normalize(
                DaemonTests
                    .Direct(fixture.Root, ["recall", "--limit", "1", "--json", "--", "cache"])
                    .Output
            ),
            Normalize(await bridge.Ok("sermofur_recall", ("question", "cache"), ("limit", 1)))
        );
        JsonElement context = JsonDocument.Parse(await bridge.Ok("sermofur_context")).RootElement;
        Assert.Equal(fixture.Root, context.GetProperty("root").GetString());
        Assert.Equal(1, context.GetProperty("counts").GetProperty("claims").GetInt32());
    }

    [Fact]
    public async Task SiblingObjectsStayInvisible()
    {
        using TestInstance fixture = new TestInstance();
        string clientA;
        string clientB;
        using (SqliteStore store = fixture.Open())
        {
            clientA = fixture.Client(store, "client-a");
            clientB = fixture.Client(store, "client-b");
        }
        string hidden = Id(
            DaemonTests
                .Direct(
                    clientB,
                    ["claim", "add", "secret of client b", "--origin", "user", "--json"]
                )
                .Output
        );
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using Bridge bridge = await Bridge.Open(daemon, clientA);
        Assert.StartsWith(
            "not_found:",
            await bridge.Error("sermofur_challenge", ("claimId", hidden))
        );
        Assert.StartsWith(
            "not_found:",
            await bridge.Error("sermofur_feedback", ("targetId", hidden), ("verdict", "wrong"))
        );
        Assert.StartsWith(
            "not_found:",
            await bridge.Error(
                "sermofur_evidence",
                ("claimId", hidden),
                ("kind", "llm_assertion"),
                ("reference", "r"),
                ("lineage", "l")
            )
        );
        Assert.DoesNotContain(
            hidden,
            await bridge.Ok("sermofur_recall", ("question", "secret client"))
        );
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error("sermofur_status", ("path", clientB))
        );
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error("sermofur_recall", ("question", "x"), ("scope", "client-b"))
        );
    }

    [Fact]
    public async Task WritesComeFromTheHostAndReplaysAreIdempotent()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using Bridge bridge = await Bridge.Open(daemon, fixture.Root, "Claude Code");
        string first = Id(
            await bridge.Ok("sermofur_claim", ("text", "-- starts with dashes"), ("key", "k1"))
        );
        string again = Id(
            await bridge.Ok("sermofur_claim", ("text", "-- starts with dashes"), ("key", "k1"))
        );
        Assert.Equal(first, again);
        string helpText = Id(await bridge.Ok("sermofur_claim", ("text", "-h")));
        Assert.NotEqual(first, helpText);
        await bridge.Ok(
            "sermofur_evidence",
            ("claimId", first),
            ("kind", "source_code"),
            ("reference", "--src/a.cs"),
            ("lineage", "repo")
        );
        await bridge.Ok("sermofur_record_retex", ("event", "e"), ("impact", "i"), ("next", "n"));
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error(
                "sermofur_record_retex",
                ("event", "--e"),
                ("impact", "i"),
                ("next", "n")
            )
        );
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error("sermofur_claim", ("text", "x"), ("origin", "user"))
        );
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error("sermofur_claim", ("text", "x"), ("actor", "someone"))
        );
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error("sermofur_claim", ("text", "x"), ("category", "decisions"))
        );
        string shown = DaemonTests.Direct(fixture.Root, ["claim", "show", first, "--json"]).Output;
        Assert.Contains("claude-code", shown);
        Assert.DoesNotContain("\"high\"", shown);
        JsonElement shownClaim = JsonDocument.Parse(shown).RootElement.GetProperty("claim");
        Assert.Equal(
            ("llm", "claude-code"),
            (
                shownClaim.GetProperty("provenance").GetProperty("origin").GetString(),
                shownClaim.GetProperty("provenance").GetProperty("actor").GetString()
            )
        );
        string content = shownClaim.GetProperty("contentJson").GetString()!;
        Assert.Equal(
            "-- starts with dashes",
            JsonDocument.Parse(content).RootElement.GetProperty("text").GetString()
        );
    }

    [Fact]
    public async Task FeedbackIsAMarkedDraftRetexWithoutEffect()
    {
        using TestInstance fixture = new TestInstance();
        string claim = Id(
            DaemonTests
                .Direct(
                    fixture.Root,
                    ["claim", "add", "queues retry three times", "--origin", "user", "--json"]
                )
                .Output
        );
        string before = DaemonTests
            .Direct(fixture.Root, ["recall", "--json", "--", "queues retry"])
            .Output;
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using Bridge bridge = await Bridge.Open(daemon, fixture.Root);
        string retex = await bridge.Ok(
            "sermofur_feedback",
            ("targetId", claim),
            ("verdict", "wrong"),
            ("comment", "it retries five times")
        );
        Assert.Contains($"feedback wrong on claim {claim}", retex);
        Assert.StartsWith(
            "invalid_input:",
            await bridge.Error("sermofur_feedback", ("targetId", claim), ("verdict", "great"))
        );
        string after = DaemonTests
            .Direct(fixture.Root, ["recall", "--json", "--", "queues retry"])
            .Output;
        // Adding any document changes BM25 statistics, so scores move; rank and confidence do not.
        JsonElement firstBefore = JsonDocument.Parse(before).RootElement.GetProperty("results")[0];
        JsonElement firstAfter = JsonDocument.Parse(after).RootElement.GetProperty("results")[0];
        Assert.Contains(claim, firstBefore.GetRawText());
        Assert.Contains(claim, firstAfter.GetRawText());
        Assert.Equal(
            firstBefore.GetProperty("confidence").GetRawText(),
            firstAfter.GetProperty("confidence").GetRawText()
        );
    }

    [Fact]
    public async Task NotRegisteredAndOutsideAnInstanceAreNamed()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        await using (Bridge bridge = await Bridge.Open(daemon, fixture.Root))
        {
            string error = await bridge.Error("sermofur_status");
            Assert.StartsWith("not_served:", error);
            Assert.Contains("smf daemon register", error);
        }
        string outside = Directory
            .CreateDirectory(
                Path.Combine(TestInstance.TempRoot, "smf-out-" + Guid.NewGuid().ToString("N")[..8])
            )
            .FullName;
        try
        {
            await using Bridge bridge = await Bridge.Open(daemon, outside);
            Assert.StartsWith("no_instance:", await bridge.Error("sermofur_status"));
        }
        finally
        {
            Directory.Delete(outside);
        }
    }

    [Fact]
    public async Task OtherVersionAndRestartedDaemon()
    {
        using TestInstance fixture = new TestInstance();
        await using (TestDaemon other = TestDaemon.Start(version: "0.0.0-other"))
        {
            await using Bridge stale = await Bridge.Open(other, fixture.Root);
            Assert.StartsWith("daemon_version_mismatch:", await stale.Error("sermofur_status"));
        }
        string home = TestDaemon.NewHome();
        DaemonPaths paths = TestDaemon.PathsOf(home);
        await using DaemonChannel channel = new DaemonChannel(
            paths,
            ProductVersion.Current,
            fixture.Root
        );
        McpBridge bridge = new McpBridge(channel, ProductVersion.Current);
        try
        {
            await using (TestDaemon first = TestDaemon.StartAt(home))
            {
                first.Registry.Register(fixture.Root, DateTimeOffset.Now);
                Assert.NotEqual(true, (await Status(bridge)).IsError);
                // Parallel calls of the host share one session without mixing answers.
                CallToolResult[] parallel = await Task.WhenAll(
                    Enumerable.Range(0, 10).Select(_ => Status(bridge).AsTask())
                );
                Assert.All(parallel, result => Assert.NotEqual(true, result.IsError));
            }
            CallToolResult stopped = await Status(bridge);
            Assert.StartsWith("daemon_unavailable:", Text(stopped));
            await using (TestDaemon second = TestDaemon.StartAt(home))
            {
                // Same bridge, new daemon: the session is opened again without restarting the bridge.
                Assert.NotEqual(true, (await Status(bridge)).IsError);
            }
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    private static ValueTask<CallToolResult> Status(McpBridge bridge) =>
        bridge.CallAsync("sermofur_status", null, "t", CancellationToken.None);

    [Fact]
    public void ToolsAreNamedAndSessionFolderAndActorAreDerived()
    {
        Assert.Equal(ToolNames, Tools.All.Select(tool => tool.Name).Order(StringComparer.Ordinal));
        Assert.Equal("claude-code", Session.Actor("Claude Code"));
        Assert.Equal("mcp-client", Session.Actor("  ***  "));
        Assert.Equal("mcp-client", Session.Actor(null));
        Assert.Equal(64, Session.Actor(new string('a', 100)).Length);
        string project = TestInstance.TempRoot;
        Assert.Equal(
            Path.TrimEndingDirectorySeparator(project),
            Session.LaunchFolder(
                name => name == Session.ProjectDirectoryVariable ? project : null,
                "/elsewhere"
            )
        );
        string current = Path.TrimEndingDirectorySeparator(Path.GetFullPath(Path.GetTempPath()));
        string expected = Path.TrimEndingDirectorySeparator(LongPath.Of(current));
        Assert.Equal(
            expected,
            Session.LaunchFolder(
                name => name == Session.ProjectDirectoryVariable ? "relative" : null,
                current
            )
        );
        Assert.Equal(expected, Session.LaunchFolder(_ => null, current));
        if (OperatingSystem.IsWindows())
        {
            // %TEMP% is often given in its 8.3 short form: it comes back in the long form.
            Assert.DoesNotContain('~', Session.LaunchFolder(_ => null, Path.GetTempPath()));
        }
    }

    private static async Task<McpClient> Client(string workingDirectory, string home)
    {
        ProcessStartInfo cli = TestInstance.CliStart(["mcp", "serve"]);
        StdioClientTransport transport = new StdioClientTransport(
            new StdioClientTransportOptions
            {
                Name = "sermofur",
                Command = cli.FileName,
                Arguments = [.. cli.ArgumentList],
                WorkingDirectory = workingDirectory,
                EnvironmentVariables = new Dictionary<string, string?>
                {
                    [DaemonPaths.HomeVariable] = home,
                    [Session.ProjectDirectoryVariable] = null,
                },
            }
        );
        return await McpClient.CreateAsync(
            transport,
            new McpClientOptions
            {
                ClientInfo = new Implementation { Name = "Claude Code Test", Version = "1.0" },
            }
        );
    }

    private static string Text(CallToolResult result) =>
        string.Concat(result.Content.OfType<TextContentBlock>().Select(block => block.Text));

    private static string Normalize(string json) =>
        JsonSerializer.Serialize(JsonDocument.Parse(json).RootElement);

    private static string Id(string json) =>
        JsonDocument.Parse(json).RootElement.GetProperty("id").GetString()!;

    /// <summary>The bridge in this process, on a test daemon.</summary>
    private sealed class Bridge : IAsyncDisposable
    {
        private readonly DaemonChannel channel;
        private readonly McpBridge bridge;
        private readonly string actor;

        private Bridge(DaemonChannel channel, string actor)
        {
            this.channel = channel;
            this.actor = actor;
            bridge = new McpBridge(channel, ProductVersion.Current);
        }

        public static Task<Bridge> Open(
            TestDaemon daemon,
            string folder,
            string host = "test host"
        ) =>
            Task.FromResult(
                new Bridge(
                    new DaemonChannel(daemon.Paths, ProductVersion.Current, folder),
                    Session.Actor(host)
                )
            );

        public async Task<string> Ok(string tool, params (string Name, object Value)[] arguments)
        {
            CallToolResult result = await Call(tool, arguments);
            Assert.True(result.IsError != true, Text(result));
            return Text(result);
        }

        public async Task<string> Error(string tool, params (string Name, object Value)[] arguments)
        {
            CallToolResult result = await Call(tool, arguments);
            Assert.True(result.IsError == true, Text(result));
            return Text(result);
        }

        public ValueTask DisposeAsync() => channel.DisposeAsync();

        private async Task<CallToolResult> Call(
            string tool,
            (string Name, object Value)[] arguments
        ) =>
            await bridge.CallAsync(
                tool,
                arguments.ToDictionary(
                    pair => pair.Name,
                    pair => JsonSerializer.SerializeToElement(pair.Value)
                ),
                actor,
                CancellationToken.None
            );
    }
}
