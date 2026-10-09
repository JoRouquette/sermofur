using System.Diagnostics;
using System.Runtime.Versioning;
using System.Text.Json;
using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class DaemonTests
{
    [Fact]
    public async Task HelloIsWelcomedAndStatusIsReported()
    {
        await using TestDaemon daemon = TestDaemon.Start();
        await using DaemonClient client = (await daemon.Connect(TestInstance.TempRoot))!;
        Assert.Equal(ProductVersion.Current, client.DaemonVersion);
        IpcMessage status = await client.StatusAsync(CancellationToken.None);
        Assert.Equal(
            (MessageKind.Status, 0, 1),
            (status.Kind, status.InstancesOpen, status.Clients)
        );
        Assert.NotNull(status.StartedAt);
    }

    [Fact]
    public async Task OtherVersionIsRefusedWithTheRemedy()
    {
        await using TestDaemon daemon = TestDaemon.Start();
        SermofurException refusal = await Assert.ThrowsAsync<SermofurException>(() =>
            daemon.Connect(TestInstance.TempRoot, "0.0.0-other")
        );
        Assert.Equal(("daemon_version_mismatch", 3), (refusal.Code, refusal.ExitCode));
        Assert.Contains("smf daemon restart", refusal.Message);
    }

    [Fact]
    public async Task NoDaemonMeansNoClient()
    {
        DaemonPaths paths = TestDaemon.PathsOf(TestDaemon.NewHome());
        Assert.Equal(EndpointState.Absent, IpcEndpoint.Inspect(paths, new FileOwnership()));
        Stopwatch watch = Stopwatch.StartNew();
        Assert.Null(
            await DaemonClient.ConnectAsync(
                paths,
                new FileOwnership(),
                ProductVersion.Current,
                TestInstance.TempRoot,
                TimeSpan.FromSeconds(5),
                CancellationToken.None
            )
        );
        Assert.True(watch.ElapsedMilliseconds < 100, $"{watch.ElapsedMilliseconds} ms");
    }

    [Fact]
    public async Task SecondDaemonOnTheSameEndpointIsRefused()
    {
        await using TestDaemon daemon = TestDaemon.Start();
        SermofurException refusal = await Assert.ThrowsAsync<SermofurException>(async () =>
        {
            await using IpcListener second = await IpcEndpoint.ListenAsync(
                daemon.Paths,
                new FileOwnership(),
                CancellationToken.None
            );
        });
        Assert.Equal("daemon_already_running", refusal.Code);
    }

    [Fact]
    public async Task UnregisteredInstanceIsNotServed()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        IpcMessage answer = await client.RunAsync(["status", "--json"], CancellationToken.None);
        Assert.Equal(MessageKind.NotServing, answer.Kind);
    }

    [Fact]
    public async Task RegisteredInstanceGivesTheOutputOfTheDirectCli()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        string[] write = ["claim", "add", "served by the daemon", "--origin", "user", "--json"];
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        IpcMessage added = await client.RunAsync(write, CancellationToken.None);
        Assert.Equal((MessageKind.Result, 0), (added.Kind, added.ExitCode));
        foreach (
            string[] read in new[]
            {
                new[] { "claim", "list", "--json" },
                ["nope"],
                ["scope", "tree"],
            }
        )
        {
            IpcMessage served = await client.RunAsync(read, CancellationToken.None);
            CliResult direct = Direct(fixture.Root, read);
            Assert.Equal(
                (direct.ExitCode, direct.Output, direct.Error),
                (served.ExitCode!.Value, served.Stdout, served.Stderr)
            );
        }
        IpcMessage status = await client.StatusAsync(CancellationToken.None);
        Assert.Equal(1, status.InstancesOpen);
    }

    [Fact]
    public async Task RelativePathIsResolvedFromTheClientFolder()
    {
        using TestInstance fixture = new TestInstance();
        string sub = Directory.CreateDirectory(Path.Combine(fixture.Root, "sub")).FullName;
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient client = (await daemon.Connect(sub))!;
        IpcMessage served = await client.RunAsync(
            ["root", "--path", "..", "--json"],
            CancellationToken.None
        );
        Assert.Equal(
            fixture.Root,
            JsonDocument.Parse(served.Stdout!).RootElement.GetProperty("root").GetString()
        );
    }

    [Fact]
    public async Task RunWithAnotherFolderThanTheHelloIsAProtocolError()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        await using Stream stream = (
            await IpcEndpoint.ConnectAsync(
                daemon.Paths,
                new FileOwnership(),
                TimeSpan.FromSeconds(5),
                CancellationToken.None
            )
        )!;
        await Framing.WriteAsync(stream, Hello(fixture.Root), CancellationToken.None);
        Assert.Equal(
            MessageKind.Welcome,
            (await Framing.ReadAsync(stream, CancellationToken.None))!.Kind
        );
        await Framing.WriteAsync(
            stream,
            new IpcMessage
            {
                Kind = MessageKind.Run,
                Id = 1,
                Argv = ["status"],
                Cwd = TestInstance.TempRoot,
            },
            CancellationToken.None
        );
        IpcMessage answer = (await Framing.ReadAsync(stream, CancellationToken.None))!;
        Assert.Equal(("error", "protocol_error"), (answer.Kind, answer.Code));
    }

    [UnixFact]
    [UnsupportedOSPlatform("windows")]
    public async Task RuntimeFolderOpenToOthersIsForeign()
    {
        await using TestDaemon daemon = TestDaemon.Start();
        string folder = daemon.Paths.RuntimeDirectory!;
        File.SetUnixFileMode(folder, File.GetUnixFileMode(folder) | UnixFileMode.OtherExecute);
        Assert.Equal(EndpointState.Foreign, IpcEndpoint.Inspect(daemon.Paths, new FileOwnership()));
        SermofurException refusal = await Assert.ThrowsAsync<SermofurException>(() =>
            daemon.Connect(TestInstance.TempRoot)
        );
        Assert.Equal(("foreign_endpoint", 4), (refusal.Code, refusal.ExitCode));
        File.SetUnixFileMode(
            folder,
            UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute
        );
    }

    [UnixFact]
    public async Task RuntimeFolderOfAnotherAccountIsForeign()
    {
        await using TestDaemon daemon = TestDaemon.Start();
        string folder = daemon.Paths.RuntimeDirectory!;
        Assert.True(
            await Sudo("-n", "true"),
            "This test needs passwordless sudo to give the folder to root."
        );
        try
        {
            Assert.True(await Sudo("-n", "chown", "0", folder));
            Assert.Equal(
                EndpointState.Foreign,
                IpcEndpoint.Inspect(daemon.Paths, new FileOwnership())
            );
            Assert.Equal(
                "foreign_endpoint",
                (
                    await Assert.ThrowsAsync<SermofurException>(() =>
                        daemon.Connect(TestInstance.TempRoot)
                    )
                ).Code
            );
        }
        finally
        {
            await Sudo("-n", "chown", Environment.UserName, folder);
        }
    }

    [Fact]
    public async Task RealProcessServesAndStopsOnSignal()
    {
        string home = TestDaemon.NewHome();
        ProcessStartInfo start = TestInstance.CliStart(["daemon", "run", "--json"]);
        start.Environment[DaemonPaths.HomeVariable] = home;
        start.RedirectStandardInput = true;
        using Process process = Process.Start(start)!;
        try
        {
            Task<string?> line = process.StandardOutput.ReadLineAsync();
            Assert.True(
                await Task.WhenAny(line, Task.Delay(TimeSpan.FromSeconds(20))) == line,
                "The daemon did not start."
            );
            JsonElement started = JsonDocument.Parse((await line)!).RootElement;
            DaemonPaths paths = TestDaemon.PathsOf(home);
            Assert.Equal(paths.Endpoint, started.GetProperty("listening").GetString());
            await using DaemonClient client = (
                await DaemonClient.ConnectAsync(
                    paths,
                    new FileOwnership(),
                    ProductVersion.Current,
                    TestInstance.TempRoot,
                    TimeSpan.FromSeconds(5),
                    CancellationToken.None
                )
            )!;
            Assert.Equal(ProductVersion.Current, client.DaemonVersion);
            CliResult second = await RunWithHome(home, "daemon", "run");
            Assert.Equal(3, second.ExitCode);
            Assert.Contains("daemon_already_running", second.Error);
        }
        finally
        {
            process.Kill(true);
            await process.WaitForExitAsync();
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public async Task RegistryCommandsWorkInARealProcess()
    {
        using TestInstance fixture = new TestInstance();
        string home = TestDaemon.NewHome();
        try
        {
            CliResult registered = await RunWithHome(
                home,
                "daemon",
                "register",
                "--path",
                fixture.Root,
                "--json"
            );
            Assert.Equal(0, registered.ExitCode);
            Assert.Equal(
                fixture.Root,
                JsonDocument.Parse(registered.Output).RootElement.GetProperty("root").GetString()
            );
            CliResult listed = await RunWithHome(home, "daemon", "instances", "--json");
            Assert.Equal(1, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
            Assert.Equal(
                0,
                (await RunWithHome(home, "daemon", "unregister", "--path", fixture.Root)).ExitCode
            );
            CliResult again = await RunWithHome(
                home,
                "daemon",
                "unregister",
                "--path",
                fixture.Root,
                "--json"
            );
            Assert.Equal(1, again.ExitCode);
            Assert.Equal(
                "not_registered",
                JsonDocument.Parse(again.Error).RootElement.GetProperty("code").GetString()
            );
            CliResult unknown = await RunWithHome(home, "daemon", "nope");
            Assert.Equal(1, unknown.ExitCode);
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    internal static IpcMessage Hello(string cwd, string? version = null) =>
        new IpcMessage
        {
            Kind = MessageKind.Hello,
            Protocol = IpcMessage.CurrentProtocol,
            ToolVersion = version ?? ProductVersion.Current,
            Cwd = cwd,
            Pid = Environment.ProcessId,
        };

    internal static CliResult Direct(string workingDirectory, string[] arguments)
    {
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        int code = new CommandRunner(output, error).Run(arguments, workingDirectory);
        return new CliResult(code, output.ToString(), error.ToString());
    }

    internal static async Task<CliResult> RunWithHome(string home, params string[] arguments)
    {
        ProcessStartInfo start = TestInstance.CliStart(arguments);
        start.Environment[DaemonPaths.HomeVariable] = home;
        using Process process = Process.Start(start)!;
        Task<string> output = process.StandardOutput.ReadToEndAsync();
        Task<string> error = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(30));
        return new CliResult(process.ExitCode, await output, await error);
    }

    private static async Task<bool> Sudo(params string[] arguments)
    {
        try
        {
            using Process process = Process.Start(
                new ProcessStartInfo("sudo", arguments) { RedirectStandardError = true }
            )!;
            await process.WaitForExitAsync();
            return process.ExitCode == 0;
        }
        catch (System.ComponentModel.Win32Exception)
        {
            return false;
        }
    }
}
