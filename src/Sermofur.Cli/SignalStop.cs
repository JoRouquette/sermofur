using System.Runtime.InteropServices;

namespace Sermofur.Cli;

/// <summary>
/// SIGTERM and SIGINT (Ctrl+C) for the long-running commands: the first signal asks for a clean
/// stop, a second one lets the process end at once.
/// </summary>
public static class SignalStop
{
    /// <summary>Registers both signals; dispose to unregister.</summary>
    public static IDisposable Register(CancellationTokenSource stop, Action? onFirst)
    {
        PosixSignalRegistration terminate = PosixSignalRegistration.Create(
            PosixSignal.SIGTERM,
            context => context.Cancel = Handle(stop, onFirst)
        );
        PosixSignalRegistration interrupt = PosixSignalRegistration.Create(
            PosixSignal.SIGINT,
            context => context.Cancel = Handle(stop, onFirst)
        );
        return new Registrations(terminate, interrupt);
    }

    /// <summary>
    /// True (cancel the default action) for the first signal, which requests the stop; false for
    /// the next ones, so that the default action ends the process.
    /// </summary>
    public static bool Handle(CancellationTokenSource stop, Action? onFirst)
    {
        if (stop.IsCancellationRequested)
        {
            return false;
        }
        onFirst?.Invoke();
        stop.Cancel();
        return true;
    }

    private sealed class Registrations(params IDisposable[] items) : IDisposable
    {
        public void Dispose()
        {
            foreach (IDisposable item in items)
            {
                item.Dispose();
            }
        }
    }
}
