using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using FsCheck;
using FsCheck.Fluent;
using FsCheck.Xunit;
using Sermofur.Domain;
using Sermofur.Mcp;

namespace Sermofur.Tests;

public class McpConfigFileTests
{
    private static readonly JsonObject Entry = McpConfigFile.Entry("smf", ["mcp", "serve"]);

    private const string Others = """
        {
          "mcpServers": {
            "github": {
              "type": "http",   "url": "https://example.com/mcp"
            },
            "files": {"command": "npx", "args": ["-y", "pkg"]}
          },
          "extra": [1, 2]
        }
        """;

    [Fact]
    public void AbsentFileGetsOnlyTheEntry()
    {
        byte[] created = McpConfigFile.WithEntry(null, Entry)!;
        JsonNode parsed = JsonNode.Parse(created)!;
        Assert.Single(parsed["mcpServers"]!.AsObject());
        Assert.True(JsonNode.DeepEquals(Entry, parsed["mcpServers"]!["sermofur"]));
        Assert.Null(McpConfigFile.WithEntry(created, Entry));
    }

    [Fact]
    public void OtherServersStayByteForByteThroughInstallUpdateAndRemoval()
    {
        byte[] original = Encoding.UTF8.GetBytes(Others);
        byte[] installed = McpConfigFile.WithEntry(original, Entry)!;
        AssertOthersKept(installed);
        Assert.Null(McpConfigFile.WithEntry(installed, Entry));
        byte[] updated = McpConfigFile.WithEntry(
            installed,
            McpConfigFile.Entry("/opt/smf", ["mcp", "serve"])
        )!;
        AssertOthersKept(updated);
        Assert.Equal(
            "/opt/smf",
            McpConfigFile.CurrentEntry(updated)!["command"]!.GetValue<string>()
        );
        byte[] removed = McpConfigFile.WithoutEntry(updated)!;
        Assert.Equal(Others, Encoding.UTF8.GetString(removed));
        Assert.Null(McpConfigFile.WithoutEntry(removed));
    }

    [Theory]
    [InlineData("{}")]
    [InlineData("{ \"mcpServers\": {} }")]
    [InlineData("{\"other\": true}")]
    [InlineData("﻿{\"mcpServers\": {\"a\": {}}}")]
    public void EntryIsAddedToAnyValidShape(string content)
    {
        byte[] result = McpConfigFile.WithEntry(Encoding.UTF8.GetBytes(content), Entry)!;
        Assert.NotNull(McpConfigFile.CurrentEntry(result));
        byte[] removed = McpConfigFile.WithoutEntry(result)!;
        Assert.Null(McpConfigFile.CurrentEntry(removed));
        JsonNode.Parse(
            removed
                .AsSpan(removed.AsSpan().StartsWith((byte[])[0xEF, 0xBB, 0xBF]) ? 3 : 0)
                .ToArray()
        );
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("[]")]
    [InlineData("{\"mcpServers\": []}")]
    [InlineData("{\"a\": 1} trailing")]
    [InlineData("{\"a\": 1 // comment\n}")]
    public void InvalidFilesAreRefused(string content)
    {
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            McpConfigFile.WithEntry(Encoding.UTF8.GetBytes(content), Entry)
        );
        Assert.Equal(("invalid_mcp_config", 3), (refusal.Code, refusal.ExitCode));
    }

    [Fact]
    public void InstallAndUninstallTouchTheFileOnlyWhenNeeded()
    {
        string folder = Directory
            .CreateDirectory(
                Path.Combine(TestInstance.TempRoot, "smf-mcp-" + Guid.NewGuid().ToString("N")[..8])
            )
            .FullName;
        try
        {
            string file = Path.Combine(folder, McpConfigFile.FileName);
            File.WriteAllText(file, "not json");
            Assert.Equal(
                "invalid_mcp_config",
                Assert.Throws<SermofurException>(() => McpDeclaration.Install(folder)).Code
            );
            Assert.Equal("not json", File.ReadAllText(file));
            File.WriteAllText(file, Others);
            DeclarationReport first = McpDeclaration.Install(folder);
            Assert.True(first.Changed);
            DateTime written = File.GetLastWriteTimeUtc(file);
            Assert.False(McpDeclaration.Install(folder).Changed);
            Assert.Equal(written, File.GetLastWriteTimeUtc(file));
            Assert.True(McpDeclaration.IsDeclared(folder));
            Assert.True(McpDeclaration.Uninstall(folder).Changed);
            Assert.Equal(Others, File.ReadAllText(file));
            Assert.False(McpDeclaration.IsDeclared(folder));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    /// <summary>Whatever the other servers, installing then removing gives back the same bytes.</summary>
    [Property(MaxTest = 200)]
    public Property InstallThenRemoveIsTheIdentity(NonEmptyString[] names, bool spaced)
    {
        string[] keys =
        [
            .. names.Select(name => name.Get).Where(name => name != "sermofur").Distinct(),
        ];
        JsonObject servers = [];
        foreach (string key in keys)
        {
            servers[key] = new JsonObject { ["command"] = key };
        }
        string content = new JsonObject { ["mcpServers"] = servers }.ToJsonString(
            new JsonSerializerOptions { WriteIndented = spaced }
        );
        byte[] original = Encoding.UTF8.GetBytes(content);
        byte[] removed = McpConfigFile.WithoutEntry(McpConfigFile.WithEntry(original, Entry)!)!;
        return removed.SequenceEqual(original).ToProperty();
    }

    [Fact]
    public void DoctorReportsTheDeclarationAndCliCommandsWork()
    {
        using TestInstance fixture = new TestInstance();
        Assert.Equal("warning", McpCheck(fixture.Root));
        CliResult installed = DaemonTests.Direct(fixture.Root, ["mcp", "install", "--json"]);
        Assert.Equal(0, installed.ExitCode);
        JsonElement report = JsonDocument.Parse(installed.Output).RootElement;
        Assert.True(report.GetProperty("changed").GetBoolean());
        Assert.Contains(
            report.GetProperty("next").EnumerateArray(),
            step => step.GetString()!.Contains("smf daemon")
        );
        Assert.Equal("ok", McpCheck(fixture.Root));
        Assert.Equal(0, DaemonTests.Direct(fixture.Root, ["mcp", "uninstall"]).ExitCode);
        Assert.Equal("warning", McpCheck(fixture.Root));
        Assert.Equal(1, DaemonTests.Direct(fixture.Root, ["mcp", "nope"]).ExitCode);
    }

    private static string McpCheck(string root)
    {
        CliResult doctor = DaemonTests.Direct(root, ["doctor", "--json"]);
        return JsonDocument
            .Parse(doctor.Output)
            .RootElement.GetProperty("checks")
            .EnumerateArray()
            .Single(check => check.GetProperty("name").GetString() == "mcp")
            .GetProperty("status")
            .GetString()!;
    }

    private static void AssertOthersKept(byte[] content)
    {
        string text = Encoding.UTF8.GetString(content);
        Assert.Contains(
            "\"github\": {\n      \"type\": \"http\",   \"url\": \"https://example.com/mcp\"\n    }",
            text.Replace("\r\n", "\n")
        );
        Assert.Contains("\"files\": {\"command\": \"npx\", \"args\": [\"-y\", \"pkg\"]}", text);
        Assert.Contains("\"extra\": [1, 2]", text);
    }
}
