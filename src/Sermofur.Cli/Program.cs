using System.Text;
using Sermofur.Cli;

UTF8Encoding utf8 = new(encoderShouldEmitUTF8Identifier: false);
Encoding? consoleEncoding = SwitchConsoleToUtf8(utf8);
ConsoleCancelEventHandler restoreOnCancel = (_, _) => RestoreConsole(consoleEncoding);
Console.CancelKeyPress += restoreOnCancel;
try
{
    // stdout and stderr explicitly in UTF-8, without BOM, whatever the original console.
    using StreamWriter output = new(Console.OpenStandardOutput(), utf8) { AutoFlush = true };
    using StreamWriter error = new(Console.OpenStandardError(), utf8) { AutoFlush = true };
    // Long form of the folder: a short 8.3 name would not match the registry of the daemon.
    return new CliRouter(output, error).Run(
        args,
        Sermofur.Daemon.LongPath.Of(Directory.GetCurrentDirectory())
    );
}
finally
{
    Console.CancelKeyPress -= restoreOnCancel;
    RestoreConsole(consoleEncoding);
}

// Switches as soon as stdout or stderr reaches the console: without it, accents are shown in
// the OEM code page. When both streams are redirected, the caller's code page, shared with it,
// is left untouched.
static Encoding? SwitchConsoleToUtf8(Encoding utf8)
{
    if (Console.IsOutputRedirected && Console.IsErrorRedirected)
    {
        return null;
    }
    try
    {
        Encoding original = Console.OutputEncoding;
        Console.OutputEncoding = utf8;
        return original;
    }
    catch (IOException)
    {
        // No console attached: the streams are still written in UTF-8 by the StreamWriters.
        return null;
    }
}

// Called on normal exit, on handled error and on Ctrl+C; termination is not cancelled.
static void RestoreConsole(Encoding? original)
{
    if (original is null)
    {
        return;
    }
    try
    {
        Console.OutputEncoding = original;
    }
    catch (IOException)
    {
        // Console detached in the meantime: nothing to restore.
    }
}
