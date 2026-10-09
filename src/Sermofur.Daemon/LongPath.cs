using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;

namespace Sermofur.Daemon;

/// <summary>
/// The form of a folder the daemon compares with its registry. On Windows a folder can be named by
/// its 8.3 short form (<c>C:\Users\JONATH~1.ROU\…</c>, as in <c>%TEMP%</c>): clients expand it to
/// the long form before sending it, or a registered instance would not be recognized. Elsewhere the
/// path is returned unchanged.
/// </summary>
public static class LongPath
{
    public static string Of(string path)
    {
        string full = Path.GetFullPath(path);
        if (!OperatingSystem.IsWindows() || !full.Contains('~'))
        {
            return full;
        }
        return Expand(full) ?? full;
    }

    [SupportedOSPlatform("windows")]
    private static string? Expand(string path)
    {
        StringBuilder buffer = new StringBuilder(1024);
        uint length = GetLongPathName(path, buffer, (uint)buffer.Capacity);
        if (length > buffer.Capacity)
        {
            buffer.Capacity = (int)length;
            length = GetLongPathName(path, buffer, (uint)buffer.Capacity);
        }
        return length == 0 ? null : buffer.ToString();
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint GetLongPathName(
        string shortPath,
        StringBuilder longPath,
        uint length
    );
}
