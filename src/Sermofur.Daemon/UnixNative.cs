using System.Net.Sockets;
using System.Runtime.InteropServices;

namespace Sermofur.Daemon;

/// <summary>
/// The few C library calls the daemon needs on Linux and macOS. glibc is loaded as
/// <c>libc.so.6</c> (libc.so is a linker script on most distributions), as in FileOwnership.
/// </summary>
internal static class UnixNative
{
    private const int LinuxSolSocket = 1;
    private const int LinuxSoPeerCred = 17;

    /// <summary>Effective user ID of the current process.</summary>
    public static uint EffectiveUserId() =>
        OperatingSystem.IsLinux() ? LinuxGetEffectiveUserId() : MacGetEffectiveUserId();

    /// <summary>
    /// User ID of the process at the other end of a connected Unix domain socket, as recorded
    /// by the kernel (<c>SO_PEERCRED</c> on Linux, <c>getpeereid</c> on macOS); null when it
    /// cannot be read, which callers treat as a foreign peer.
    /// </summary>
    public static uint? PeerUserId(Socket socket)
    {
        int descriptor = (int)socket.SafeHandle.DangerousGetHandle();
        if (OperatingSystem.IsLinux())
        {
            // struct ucred { pid_t pid; uid_t uid; gid_t gid; }: three 32-bit fields.
            int[] credentials = new int[3];
            int length = sizeof(int) * 3;
            return
                LinuxGetSocketOption(
                    descriptor,
                    LinuxSolSocket,
                    LinuxSoPeerCred,
                    credentials,
                    ref length
                ) == 0
                && length == sizeof(int) * 3
                ? unchecked((uint)credentials[1])
                : null;
        }
        if (OperatingSystem.IsMacOS())
        {
            return MacGetPeerId(descriptor, out uint user, out uint _) == 0 ? user : null;
        }
        return null;
    }

    [DllImport("libc.so.6", EntryPoint = "geteuid")]
    private static extern uint LinuxGetEffectiveUserId();

    [DllImport("libc", EntryPoint = "geteuid")]
    private static extern uint MacGetEffectiveUserId();

    [DllImport("libc.so.6", EntryPoint = "getsockopt", SetLastError = true)]
    private static extern int LinuxGetSocketOption(
        int socket,
        int level,
        int option,
        [Out] int[] value,
        ref int length
    );

    [DllImport("libc", EntryPoint = "getpeereid", SetLastError = true)]
    private static extern int MacGetPeerId(int socket, out uint user, out uint group);
}
