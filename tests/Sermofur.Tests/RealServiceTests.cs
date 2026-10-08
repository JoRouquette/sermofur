using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Sermofur.Cli;
using Sermofur.Daemon;
using Xunit.Abstractions;

namespace Sermofur.Tests;

/// <summary>
/// The real service of the session (T028, SC-001, SC-002): scheduled task, systemd user unit or
/// launchd agent, at the real locations of the user, as installed by smf.
/// </summary>
[Collection("Daemon timing")]
public class RealServiceTests(ITestOutputHelper log)
{
    [ServiceFact]
    public async Task InstallRestartAfterAKillPilotAndUninstall()
    {
        using TestInstance fixture = new TestInstance();
        await Smf("daemon", "uninstall");
        try
        {
            Stopwatch install = Stopwatch.StartNew();
            JsonElement installed = await Status("daemon", "install", "--json");
            log.WriteLine($"install: {install.Elapsed.TotalSeconds:0.0} s, {installed}");
            Assert.Equal("running", installed.GetProperty("state").GetString());
            Assert.True(install.Elapsed < TimeSpan.FromMinutes(1));
            Assert.Equal(ProductVersion.Current, installed.GetProperty("version").GetString());

            Assert.Equal(0, (await Smf("daemon", "register", "--path", fixture.Root)).ExitCode);
            Dictionary<string, string> data = fixture.Snapshot();
            JsonElement served = Json(await SmfIn(fixture.Root, "status", "--json"));
            Assert.Equal("daemon", served.GetProperty("mode").GetString());

            int first = installed.GetProperty("pid").GetInt32();
            Process.GetProcessById(first).Kill();
            Stopwatch back = Stopwatch.StartNew();
            int second = await WaitForPid(first);
            log.WriteLine($"back after a kill: {back.Elapsed.TotalSeconds:0.0} s");
            Assert.True(
                back.Elapsed < TimeSpan.FromSeconds(10),
                $"{back.Elapsed.TotalSeconds:0.0} s"
            );
            Assert.NotEqual(first, second);

            Assert.Equal(
                "installed_stopped",
                (await Status("daemon", "stop", "--json")).GetProperty("state").GetString()
            );
            Assert.Equal(
                "running",
                (await Status("daemon", "start", "--json")).GetProperty("state").GetString()
            );
            Assert.Equal(
                "running",
                (await Status("daemon", "install", "--json")).GetProperty("state").GetString()
            );

            Stopwatch uninstall = Stopwatch.StartNew();
            Assert.Equal(
                "absent",
                (await Status("daemon", "uninstall", "--json")).GetProperty("state").GetString()
            );
            Assert.True(uninstall.Elapsed < TimeSpan.FromMinutes(1));
            Assert.Equal(data.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
            string instances = Encoding.UTF8.GetString(
                (await Smf("daemon", "instances", "--json")).Output
            );
            Assert.Contains(Path.GetFileName(fixture.Root), instances);
        }
        finally
        {
            await Smf("daemon", "uninstall");
            await Smf("daemon", "unregister", "--path", fixture.Root);
        }
    }

    private static async Task<int> WaitForPid(int killed)
    {
        Stopwatch waited = Stopwatch.StartNew();
        while (waited.Elapsed < TimeSpan.FromSeconds(30))
        {
            JsonElement status = Json(await Smf("daemon", "status", "--json"));
            if (
                status.GetProperty("pid") is { ValueKind: JsonValueKind.Number } pid
                && pid.GetInt32() != killed
            )
            {
                return pid.GetInt32();
            }
            await Task.Delay(200);
        }
        throw new TimeoutException("The service did not bring the daemon back.");
    }

    private static async Task<JsonElement> Status(params string[] arguments) =>
        Json(await Smf(arguments));

    private static JsonElement Json(RawCliResult result)
    {
        Assert.True(result.ExitCode == 0, Encoding.UTF8.GetString(result.Error));
        return JsonDocument.Parse(result.Output).RootElement;
    }

    private static Task<RawCliResult> Smf(params string[] arguments) =>
        SmfIn(TestInstance.TempRoot, arguments);

    /// <summary>The CLI at the real locations of the user: the isolation of the test run is lifted.</summary>
    private static async Task<RawCliResult> SmfIn(
        string workingDirectory,
        params string[] arguments
    )
    {
        ProcessStartInfo start = TestInstance.CliStart(arguments);
        start.WorkingDirectory = workingDirectory;
        start.Environment.Remove(DaemonPaths.HomeVariable);
        start.Environment.Remove(CliRouter.NoDaemonVariable);
        using Process cli = Process.Start(start)!;
        using MemoryStream output = new MemoryStream();
        using MemoryStream error = new MemoryStream();
        Task copyOut = cli.StandardOutput.BaseStream.CopyToAsync(output);
        Task copyErr = cli.StandardError.BaseStream.CopyToAsync(error);
        await cli.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(90));
        await Task.WhenAll(copyOut, copyErr);
        return new RawCliResult(cli.ExitCode, output.ToArray(), error.ToArray());
    }
}
