using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using Microsoft.Win32.SafeHandles;

namespace Sermofur.Daemon;

/// <summary>
/// <c>smf daemon run --supervise</c> (research R2): restarts the daemon within a second after an
/// abnormal exit, which the Windows task scheduler cannot do in less than a minute. A clean exit
/// (stop requested) ends the supervisor; a daemon that fails at once three times in a row (another
/// daemon runs, endpoint of another account) ends it too, instead of looping. On Windows the
/// daemon lives in a job object, so it never outlives its supervisor.
/// </summary>
public sealed class Supervisor(
    ProcessStartInfo child,
    DaemonLog log,
    Func<Task>? requestStop = null
)
{
    private static readonly TimeSpan QuickFailure = TimeSpan.FromSeconds(3);

    public TimeSpan RestartDelay { get; init; } = TimeSpan.FromMilliseconds(500);

    /// <summary>
    /// Start of the supervised daemon. A scheduled task carries no environment: the variables
    /// recorded at install time (DOTNET_ROOT, PATH) reach the daemon through its supervisor.
    /// </summary>
    public static ProcessStartInfo ChildStart(
        Services.ServiceDefinition child,
        Services.ServiceDefinition? installed
    )
    {
        ProcessStartInfo start = new ProcessStartInfo(child.Executable) { UseShellExecute = false };
        foreach (string argument in child.Arguments)
        {
            start.ArgumentList.Add(argument);
        }
        foreach (
            KeyValuePair<string, string> variable in installed?.Environment
                ?? new Dictionary<string, string>()
        )
        {
            start.Environment[variable.Key] = variable.Value;
        }
        return start;
    }

    public int Run(CancellationToken stop)
    {
        try
        {
            return Supervise(stop);
        }
        finally
        {
            log.FlushBeforeExit();
        }
    }

    private int Supervise(CancellationToken stop)
    {
        using WindowsJob? job = OperatingSystem.IsWindows() ? WindowsJob.Create() : null;
        int quickFailures = 0;
        Queue<DateTimeOffset> restarts = new Queue<DateTimeOffset>();
        while (true)
        {
            Stopwatch alive = Stopwatch.StartNew();
            using Process daemon =
                Process.Start(child)
                ?? throw new InvalidOperationException("The daemon process did not start.");
            if (OperatingSystem.IsWindows())
            {
                job!.Assign(daemon);
            }
            log.Write("supervised_start", null, daemon.Id);
            try
            {
                daemon.WaitForExitAsync(stop).GetAwaiter().GetResult();
            }
            catch (OperationCanceledException)
            {
                StopDaemon(daemon);
                return 0;
            }
            if (daemon.ExitCode == 0)
            {
                log.Write("supervised_stop");
                return 0;
            }
            log.Write(
                "supervised_exit",
                $"exit_{daemon.ExitCode}",
                daemon.Id,
                alive.ElapsedMilliseconds
            );
            quickFailures = alive.Elapsed < QuickFailure ? quickFailures + 1 : 0;
            if (quickFailures >= 3)
            {
                log.Write("supervised_give_up", $"exit_{daemon.ExitCode}");
                return daemon.ExitCode;
            }
            DateTimeOffset now = DateTimeOffset.Now;
            restarts.Enqueue(now);
            while (restarts.Count > 0 && now - restarts.Peek() > TimeSpan.FromMinutes(1))
            {
                restarts.Dequeue();
            }
            // More than five restarts in a minute: slow down rather than spin.
            TimeSpan delay = restarts.Count > 5 ? TimeSpan.FromSeconds(5) : RestartDelay;
            if (stop.WaitHandle.WaitOne(delay))
            {
                return 0;
            }
        }
    }

    private void StopDaemon(Process daemon)
    {
        try
        {
            // The daemon drains its started commands before it exits (ADR 0016).
            requestStop?.Invoke().Wait(DaemonLimits.Default.StopTimeout);
        }
        catch (AggregateException)
        {
            // The daemon may already be gone.
        }
        if (!daemon.WaitForExit(TimeSpan.FromSeconds(5)))
        {
            daemon.Kill(true);
        }
    }

    /// <summary>Job object that kills its processes when the supervisor ends, even when killed.</summary>
    [SupportedOSPlatform("windows")]
    private sealed class WindowsJob : IDisposable
    {
        private const int ExtendedLimitInformation = 9;
        private const uint KillOnJobClose = 0x2000;

        private readonly SafeFileHandle handle;

        private WindowsJob(SafeFileHandle handle) => this.handle = handle;

        public static WindowsJob Create()
        {
            SafeFileHandle handle = CreateJobObject(IntPtr.Zero, null);
            if (handle.IsInvalid)
            {
                throw new InvalidOperationException(
                    "Cannot create the job object of the supervisor."
                );
            }
            ExtendedLimit limit = new ExtendedLimit { LimitFlags = KillOnJobClose };
            if (
                !SetInformationJobObject(
                    handle,
                    ExtendedLimitInformation,
                    ref limit,
                    (uint)Marshal.SizeOf<ExtendedLimit>()
                )
            )
            {
                handle.Dispose();
                throw new InvalidOperationException(
                    "Cannot configure the job object of the supervisor."
                );
            }
            return new WindowsJob(handle);
        }

        public void Assign(Process process) => AssignProcessToJobObject(handle, process.SafeHandle);

        public void Dispose() => handle.Dispose();

        // JOBOBJECT_EXTENDED_LIMIT_INFORMATION, with the basic limits inlined.
        [StructLayout(LayoutKind.Sequential)]
        private struct ExtendedLimit
        {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint PriorityClass;
            public uint SchedulingClass;
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateJobObject(IntPtr attributes, string? name);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetInformationJobObject(
            SafeFileHandle job,
            int informationClass,
            ref ExtendedLimit information,
            uint length
        );

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(
            SafeFileHandle job,
            SafeProcessHandle process
        );
    }
}
