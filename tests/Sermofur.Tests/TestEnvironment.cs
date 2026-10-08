using System.Runtime.CompilerServices;
using Sermofur.Daemon;

namespace Sermofur.Tests;

/// <summary>
/// Isolation of the whole test run from the real daemon of the developer: every in-process
/// command and every child CLI inherits a daemon home of its own, where no daemon runs unless a
/// test starts one.
/// </summary>
internal static class TestEnvironment
{
    // CA2255 warns libraries against module initializers; this is the test assembly, which must
    // isolate itself before any test runs.
#pragma warning disable CA2255
    [ModuleInitializer]
#pragma warning restore CA2255
    internal static void IsolateFromTheRealDaemon() =>
        Environment.SetEnvironmentVariable(
            DaemonPaths.HomeVariable,
            Path.Combine(Path.GetTempPath(), $"smfd-run-{Environment.ProcessId}")
        );
}
