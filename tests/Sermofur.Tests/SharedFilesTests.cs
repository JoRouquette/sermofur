using System.Diagnostics;
using System.Text.Json;
using Sermofur.Daemon;
using Sermofur.Domain;
using Xunit.Abstractions;

namespace Sermofur.Tests;

/// <summary>
/// Several processes on one registry, and on one journal (issue #9). In the serial collection:
/// the journal test counts every line, and must not share the machine with the rest.
/// </summary>
[Collection("Daemon timing")]
public class SharedFilesTests(ITestOutputHelper report)
{
    [Fact]
    public void ConcurrentRegistrationsAreAllKept()
    {
        string home = TestDaemon.NewHome();
        List<TestInstance> fixtures = [.. Enumerable.Range(0, 8).Select(_ => new TestInstance())];
        try
        {
            string file = TestDaemon.PathsOf(home).RegistryFile;
            Parallel.ForEach(
                fixtures,
                new ParallelOptions { MaxDegreeOfParallelism = 8 },
                fixture => new InstanceRegistry(file).Register(fixture.Root, DateTimeOffset.Now)
            );
            IReadOnlyList<RegisteredInstance> entries = new InstanceRegistry(file).Read();
            Assert.Equal(
                fixtures.Select(fixture => fixture.Root).Order(StringComparer.Ordinal),
                entries.Select(entry => entry.Root).Order(StringComparer.Ordinal)
            );
        }
        finally
        {
            fixtures.ForEach(fixture => fixture.Dispose());
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public void RegistryHeldByAnotherProcessIsReportedBusy()
    {
        string home = TestDaemon.NewHome();
        using TestInstance fixture = new TestInstance();
        try
        {
            InstanceRegistry registry = new InstanceRegistry(
                TestDaemon.PathsOf(home).RegistryFile,
                lockTimeout: TimeSpan.FromMilliseconds(300)
            );
            Directory.CreateDirectory(Path.GetDirectoryName(registry.LockFile)!);
            using (FileTurn.Take(registry.LockFile))
            {
                SermofurException busy = Assert.Throws<SermofurException>(() =>
                    registry.Register(fixture.Root, DateTimeOffset.Now)
                );
                Assert.Equal(("registry_busy", 3), (busy.Code, busy.ExitCode));
            }
            registry.Register(fixture.Root, DateTimeOffset.Now);
            Assert.Single(registry.Read());
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public async Task RegistryOpenedByAReaderIsReplacedOnceTheReaderLetsGo()
    {
        string home = TestDaemon.NewHome();
        using TestInstance first = new TestInstance();
        using TestInstance second = new TestInstance();
        try
        {
            InstanceRegistry registry = new InstanceRegistry(TestDaemon.PathsOf(home).RegistryFile);
            registry.Register(first.Root, DateTimeOffset.Now);
            // A reader holds the file the way the daemon reads it, for longer than a read.
            FileStream reader = new FileStream(
                registry.File,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete
            );
            Task released = Task.Run(async () =>
            {
                await Task.Delay(150);
                await reader.DisposeAsync();
            });
            registry.Register(second.Root, DateTimeOffset.Now);
            await released.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(2, registry.Read().Count);
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public void TwoRewritesWithTheSameDateAndSizeAreBothSeen()
    {
        string home = TestDaemon.NewHome();
        using TestInstance first = new TestInstance();
        using TestInstance second = new TestInstance();
        try
        {
            InstanceRegistry registry = new InstanceRegistry(TestDaemon.PathsOf(home).RegistryFile);
            registry.Register(first.Root, DateTimeOffset.Now);
            ServingGate gate = new ServingGate(registry);
            Assert.Equal(first.Root, gate.Resolve(first.Root));
            RegistryStamp before = registry.Stamp();
            // Same length of root, same date written back: only the content changed.
            registry.Unregister(first.Root);
            registry.Register(second.Root, DateTimeOffset.Now);
            File.SetLastWriteTimeUtc(registry.File, before.WrittenUtc);
            Assert.Equal(before, registry.Stamp());
            Assert.Null(gate.Resolve(first.Root));
            Assert.Equal(second.Root, gate.Resolve(second.Root));
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public void RegistryDatedInTheFutureIsReadOnceThenByItsStamp()
    {
        string home = TestDaemon.NewHome();
        using TestInstance first = new TestInstance();
        using TestInstance second = new TestInstance();
        try
        {
            InstanceRegistry registry = new InstanceRegistry(TestDaemon.PathsOf(home).RegistryFile);
            registry.Register(first.Root, DateTimeOffset.Now);
            DateTime future = DateTime.UtcNow.AddHours(1);
            File.SetLastWriteTimeUtc(registry.File, future);
            ServingGate gate = new ServingGate(registry);
            Assert.Equal(first.Root, gate.Resolve(first.Root));
            // Rewritten with the same date and size: a future date is not "recent", so only a
            // change of stamp makes the daemon read it again.
            registry.Unregister(first.Root);
            registry.Register(second.Root, DateTimeOffset.Now);
            File.SetLastWriteTimeUtc(registry.File, future);
            Assert.Equal(first.Root, gate.Resolve(first.Root));
            File.SetLastWriteTimeUtc(registry.File, future.AddSeconds(1));
            Assert.Equal(second.Root, gate.Resolve(second.Root));
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public void JournalSharedByFourWritersKeepsEveryLineAcrossTwoRotations()
    {
        string home = TestDaemon.NewHome();
        try
        {
            string file = TestDaemon.PathsOf(home).LogFile;
            const int Writers = 4;
            const int Lines = 600;
            // The 2,400 lines (about 260 KB) are about 2.6 files of 100 KiB: two rotations with a
            // wide margin on both sides, and everything still fits the three files kept.
            const long MaxBytes = 100 * 1024;
            Parallel.For(
                0,
                Writers,
                new ParallelOptions { MaxDegreeOfParallelism = Writers },
                writer =>
                {
                    // One journal object per writer, as each process has its own.
                    DaemonLog log = new DaemonLog(file, maxFileBytes: MaxBytes);
                    for (int line = 0; line < Lines; line++)
                    {
                        log.Write(
                            $"w{writer}-{line}",
                            "padding_to_make_the_line_longer_than_usual"
                        );
                    }
                    Assert.True(log.Flush(TimeSpan.FromSeconds(30)));
                }
            );
            List<string> events = [];
            foreach (string part in new[] { file, file + ".1", file + ".2" }.Where(File.Exists))
            {
                Assert.True(new FileInfo(part).Length <= MaxBytes + 512);
                foreach (string text in File.ReadAllLines(part))
                {
                    events.Add(
                        JsonDocument.Parse(text).RootElement.GetProperty("event").GetString()!
                    );
                }
            }
            Assert.True(File.Exists(file + ".2"), "the journal did not rotate twice");
            Assert.Equal(Writers * Lines, events.Count);
            Assert.Equal(Writers * Lines, events.Distinct().Count());
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [Fact]
    public void JournalNeverHoldsTheCallerWhileAnotherProcessHasItsTurn()
    {
        string home = TestDaemon.NewHome();
        try
        {
            string file = TestDaemon.PathsOf(home).LogFile;
            FileTurn.CreatePrivateFolder(Path.GetDirectoryName(file)!);
            DaemonLog log = new DaemonLog(file);
            Stopwatch watch = Stopwatch.StartNew();
            using (FileTurn.Take(file + ".lock"))
            {
                for (int line = 0; line < 100; line++)
                {
                    log.Write($"held-{line}");
                }
                report.WriteLine($"100 lines queued in {watch.ElapsedMilliseconds} ms");
                // Waiting for the turn would cost a whole second from the first line.
                Assert.True(watch.ElapsedMilliseconds < 700, $"{watch.ElapsedMilliseconds} ms");
                // Held a while, but given back well within the second the writer waits.
                Thread.Sleep((int)Math.Max(0, 400 - watch.ElapsedMilliseconds));
            }
            // The turn came back within the second the writer waits: every line is written.
            Assert.True(log.Flush(TimeSpan.FromSeconds(10)));
            Assert.Equal(100, File.ReadAllLines(file).Length);
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [UnixFact]
    public void LockFilesAndNewFoldersArePrivate()
    {
        if (OperatingSystem.IsWindows())
        {
            // Skipped by the attribute; the guard tells the platform analyzer.
            return;
        }
        string home = TestDaemon.NewHome();
        try
        {
            string folder = Path.Combine(home, "state");
            FileTurn.CreatePrivateFolder(folder);
            string lockFile = Path.Combine(folder, "test.lock");
            using (FileTurn.Take(lockFile)) { }
            Assert.Equal(
                UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute,
                File.GetUnixFileMode(folder)
            );
            Assert.Equal(
                UnixFileMode.UserRead | UnixFileMode.UserWrite,
                File.GetUnixFileMode(lockFile)
            );
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }

    [UnixFact]
    public void LockFileLeftOpenToOthersIsNarrowed()
    {
        if (OperatingSystem.IsWindows())
        {
            // Skipped by the attribute; the guard tells the platform analyzer.
            return;
        }
        string home = TestDaemon.NewHome();
        try
        {
            Directory.CreateDirectory(home);
            // As an older version created daemon.lock: readable by everyone.
            string lockFile = Path.Combine(home, "daemon.lock");
            File.WriteAllBytes(lockFile, []);
            File.SetUnixFileMode(
                lockFile,
                UnixFileMode.UserRead
                    | UnixFileMode.UserWrite
                    | UnixFileMode.GroupRead
                    | UnixFileMode.OtherRead
            );
            using (FileTurn.Take(lockFile)) { }
            Assert.Equal(
                UnixFileMode.UserRead | UnixFileMode.UserWrite,
                File.GetUnixFileMode(lockFile)
            );
        }
        finally
        {
            TestDaemon.DeleteHome(home);
        }
    }
}
