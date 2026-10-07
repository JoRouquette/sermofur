using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Sermofur.Application;
using Sermofur.Cli;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public sealed class TestInstance : IDisposable
{
    /// <summary>
    /// The temporary directory with every link of its path resolved. Discovery refuses links on
    /// purpose, and on macOS the temporary directory lives under <c>/var</c>, itself a link to
    /// <c>/private/var</c>: tests start from the real path.
    /// </summary>
    public static string TempRoot { get; } = ResolveLinks(Path.GetTempPath());

    public string Root { get; } =
        Path.Combine(TempRoot, "sermofur-test-" + Guid.NewGuid().ToString("N"));
    public InstanceManager Manager { get; } = new();

    public TestInstance()
    {
        RequireNoEntryAboveTemp();
        Directory.CreateDirectory(Root);
        Manager.Initialize(Root);
    }

    /// <summary>
    /// Tests create their instances under the temporary directory. Discovery fails closed, so a
    /// <c>.sermofur</c> entry in one of its parents (for example an instance created in the
    /// user profile) would make many tests fail for an unrelated reason: say so explicitly.
    /// </summary>
    public static void RequireNoEntryAboveTemp()
    {
        Exception? error = Record.Exception(() => new InstanceManager().Discover(TempRoot));
        string found = error switch
        {
            null => "an instance was found",
            SermofurException sermofur => $"{sermofur.Code}: {sermofur.Message}",
            _ => error.Message,
        };
        Assert.True(
            error is SermofurException { Code: "no_instance" },
            "Tests need a temporary directory with no .sermofur entry above it and no link in its "
                + $"path; discovery from it returned {found}."
        );
    }

    public SqliteStore Open(bool readOnly = false) =>
        new(Root, Manager.ReadConfiguration(Root).InstanceId, readOnly);

    public MemoryService Memory(SqliteStore store, string? path = null) =>
        new(store, Manager.ResolveContext(path ?? Root, Root, store.ReadScopes()));

    public ScopeService Scopes(SqliteStore store, string? path = null) =>
        new(
            store,
            new LocalPathResolver(),
            Manager.ResolveContext(path ?? Root, Root, store.ReadScopes())
        );

    public string Client(SqliteStore store, string id)
    {
        string path = Path.Combine(Root, id);
        Directory.CreateDirectory(path);
        Scopes(store).Register(new Scope(id, ScopeKind.Client, ScopePolicy.WorkspaceScopeId, id));
        return path;
    }

    public static Provenance User => new(ActorKind.User, "test-user", DateTimeOffset.UtcNow);

    public static ClaimContent Fact(string text = "local hypothesis") =>
        new(text, MemoryCategory.Semantic, Volatility.Evolving);

    public Dictionary<string, string> Snapshot() => SnapshotOf(Root);

    /// <summary>Hash of every file under the <c>.sermofur</c> directory of <paramref name="root"/>.</summary>
    public static Dictionary<string, string> SnapshotOf(string root) =>
        Directory
            .GetFiles(Path.Combine(root, ".sermofur"), "*", SearchOption.AllDirectories)
            .ToDictionary(
                file => Path.GetRelativePath(root, file),
                file =>
                    Convert.ToHexString(
                        System.Security.Cryptography.SHA256.HashData(File.ReadAllBytes(file))
                    )
            );

    public void Dispose()
    {
        if (
            !Path.GetFileName(Root).StartsWith("sermofur-test-", StringComparison.Ordinal)
            || !LocalPaths.Contains(TempRoot, Root)
        )
        {
            throw new InvalidOperationException("Unsafe test cleanup");
        }
        Directory.Delete(Root, true);
    }

    /// <summary>Removes a directory link, a junction on Windows or a symbolic link elsewhere,
    /// without touching its target, even when the target no longer exists.</summary>
    public static void DeleteDirectoryLink(string link)
    {
        if (OperatingSystem.IsWindows())
        {
            Directory.Delete(link);
            return;
        }
        // On Unix the link is a file entry: unlink it. Directory.Delete fails on a dangling link
        // (DirectoryNotFoundException).
        File.Delete(link);
    }

    private static string ResolveLinks(string path, int depth = 0)
    {
        if (depth > 32)
        {
            throw new IOException($"Too many levels of links while resolving {path}.");
        }
        string full = Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
        string current = Path.GetPathRoot(full)!;
        string relative = Path.GetRelativePath(current, full);
        if (relative == ".")
        {
            return current;
        }
        foreach (
            string part in relative.Split(
                Path.DirectorySeparatorChar,
                StringSplitOptions.RemoveEmptyEntries
            )
        )
        {
            string next = Path.Combine(current, part);
            FileSystemInfo? target = new DirectoryInfo(next).ResolveLinkTarget(true);
            // The parents of a link target may be links themselves: resolve them too.
            current = target is null
                ? next
                : ResolveLinks(Path.TrimEndingDirectorySeparator(target.FullName), depth + 1);
        }
        return current;
    }

    /// <summary>In-process run of the CLI, for cases that do not require a real process.</summary>
    public static CliResult Run(params string[] arguments)
    {
        using StringWriter stdout = new();
        using StringWriter stderr = new();
        int exitCode = new CommandRunner(stdout, stderr).Run(arguments);
        return new CliResult(exitCode, stdout.ToString(), stderr.ToString());
    }

    public static string ErrorCode(CliResult result) =>
        JsonDocument.Parse(result.Error).RootElement.GetProperty("code").GetString()!;

    /// <summary>Real process run, outputs decoded as UTF-8 without stripping a BOM.</summary>
    public static async Task<CliResult> RunCli(params string[] arguments)
    {
        RawCliResult raw = await RunCliRaw(arguments);
        UTF8Encoding utf8 = new(false);
        return new CliResult(raw.ExitCode, utf8.GetString(raw.Output), utf8.GetString(raw.Error));
    }

    /// <summary>Real process run, outputs kept as the raw bytes written by the CLI.</summary>
    public static async Task<RawCliResult> RunCliRaw(params string[] arguments)
    {
        DirectoryInfo? repository = new(AppContext.BaseDirectory);
        while (
            repository is not null
            && !File.Exists(Path.Combine(repository.FullName, "Sermofur.slnx"))
        )
        {
            repository = repository.Parent;
        }
        string config = AppContext.BaseDirectory.Contains("Release", StringComparison.Ordinal)
            ? "Release"
            : "Debug";
        string dll = Path.Combine(
            repository!.FullName,
            "src",
            "Sermofur.Cli",
            "bin",
            config,
            "net10.0",
            "Sermofur.Cli.dll"
        );
        ProcessStartInfo start = new("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        start.ArgumentList.Add(dll);
        foreach (string argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }
        using Process process = Process.Start(start)!;
        // Raw streams: neither the console encoding nor BOM detection alter what the CLI wrote.
        Task<byte[]> stdout = ReadAllBytesAsync(process.StandardOutput.BaseStream);
        Task<byte[]> stderr = ReadAllBytesAsync(process.StandardError.BaseStream);
        using CancellationTokenSource timeout = new(TimeSpan.FromSeconds(30));
        try
        {
            await process.WaitForExitAsync(timeout.Token);
        }
        catch (OperationCanceledException)
        {
            process.Kill(true);
            throw;
        }
        return new RawCliResult(process.ExitCode, await stdout, await stderr);
    }

    private static async Task<byte[]> ReadAllBytesAsync(Stream stream)
    {
        using MemoryStream buffer = new();
        await stream.CopyToAsync(buffer);
        return buffer.ToArray();
    }
}

public sealed record CliResult(int ExitCode, string Output, string Error);

public sealed record RawCliResult(int ExitCode, byte[] Output, byte[] Error);
