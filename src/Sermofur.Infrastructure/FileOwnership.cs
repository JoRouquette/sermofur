using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Claims;
using System.Security.Principal;

namespace Sermofur.Infrastructure;

/// <summary>Type and owner of a file system entry, read without following links.</summary>
public sealed record EntryStatus(bool IsRegularFile, bool IsDirectory, bool IsOwnedByCurrentUser);

/// <summary>Reads the owner and type of an entry; replaceable in tests.</summary>
public interface IFileOwnership
{
    /// <summary>Status of <paramref name="path"/>, or null when it does not exist.</summary>
    EntryStatus? Inspect(string path);
}

/// <summary>
/// Operating-system implementation (ADR 0014). .NET exposes no owner on Unix: Linux uses
/// <c>statx</c>, whose structure is defined by the kernel for every architecture; macOS uses
/// <c>lstat</c> with 64-bit inodes. Windows compares the owner SID with the current user, and
/// accepts the Administrators group when the current user belongs to it (folders created from
/// an elevated session).
/// </summary>
public sealed class FileOwnership : IFileOwnership
{
    private const int AtCurrentDirectory = -100;
    private const int AtSymlinkNoFollow = 0x100;
    private const uint StatxTypeModeUid = 0x1 | 0x2 | 0x8;
    private const uint TypeMask = 0xF000;
    private const uint RegularFile = 0x8000;
    private const uint DirectoryType = 0x4000;
    private const int NoSuchEntry = 2;
    private const int NotADirectory = 20;

    // struct statx (linux/stat.h): stx_uid (__u32) at 20, stx_mode (__u16) at 28.
    private const int StatxUidOffset = 20;
    private const int StatxModeOffset = 28;

    // struct stat with 64-bit inodes (Darwin sys/stat.h): st_mode (mode_t, 16 bits) at 4,
    // st_uid (uid_t) at 16.
    private const int DarwinStatModeOffset = 4;
    private const int DarwinStatUidOffset = 16;

    public EntryStatus? Inspect(string path)
    {
        if (OperatingSystem.IsWindows())
        {
            return InspectWindows(path);
        }
        if (OperatingSystem.IsLinux())
        {
            return InspectUnix(path, linux: true);
        }
        if (OperatingSystem.IsMacOS())
        {
            return InspectUnix(path, linux: false);
        }
        throw new PlatformNotSupportedException(
            "File ownership is checked on Windows, Linux and macOS."
        );
    }

    [SupportedOSPlatform("windows")]
    private static EntryStatus? InspectWindows(string path)
    {
        FileSystemInfo entry = File.Exists(path) ? new FileInfo(path) : new DirectoryInfo(path);
        if (!entry.Exists)
        {
            return null;
        }
        FileSystemSecurity security = entry is FileInfo file
            ? file.GetAccessControl(AccessControlSections.Owner)
            : ((DirectoryInfo)entry).GetAccessControl(AccessControlSections.Owner);
        SecurityIdentifier? owner =
            security.GetOwner(typeof(SecurityIdentifier)) as SecurityIdentifier;
        using WindowsIdentity identity = WindowsIdentity.GetCurrent();
        bool owned =
            owner is not null
            && (owner == identity.User || IsAdministratorsOfUser(owner, identity));
        bool directory = (entry.Attributes & FileAttributes.Directory) != 0;
        return new EntryStatus(!directory, directory, owned);
    }

    [SupportedOSPlatform("windows")]
    private static bool IsAdministratorsOfUser(SecurityIdentifier owner, WindowsIdentity identity)
    {
        if (
            !OperatingSystem.IsWindows()
            || !owner.IsWellKnown(WellKnownSidType.BuiltinAdministratorsSid)
        )
        {
            return false;
        }
        // Without elevation the group is present for deny only; membership is what counts.
        return identity.Groups?.Contains(owner) == true
            || identity.Claims.Any(claim =>
                claim.Type == ClaimTypes.DenyOnlySid && claim.Value == owner.Value
            );
    }

    private static EntryStatus? InspectUnix(string path, bool linux)
    {
        byte[] buffer = new byte[256];
        int result;
        uint currentUser;
        try
        {
            result = linux
                ? Statx(AtCurrentDirectory, path, AtSymlinkNoFollow, StatxTypeModeUid, buffer)
                : MacLstat(path, buffer);
            currentUser = GetEffectiveUserId();
        }
        catch (Exception exception)
            when (exception is DllNotFoundException or EntryPointNotFoundException)
        {
            // musl or a glibc older than 2.28: the owner cannot be read, so it is not trusted.
            throw new IOException(
                "File owner unavailable on this C library (glibc 2.28+ required).",
                exception
            );
        }
        if (result != 0)
        {
            int error = Marshal.GetLastPInvokeError();
            if (error is NoSuchEntry or NotADirectory)
            {
                return null;
            }
            throw new IOException($"Cannot read the status of an entry (errno {error}).");
        }
        uint owner = BitConverter.ToUInt32(buffer, linux ? StatxUidOffset : DarwinStatUidOffset);
        uint mode = BitConverter.ToUInt16(buffer, linux ? StatxModeOffset : DarwinStatModeOffset);
        return new EntryStatus(
            (mode & TypeMask) == RegularFile,
            (mode & TypeMask) == DirectoryType,
            owner == currentUser
        );
    }

    private static int MacLstat(string path, byte[] buffer) =>
        RuntimeInformation.ProcessArchitecture == Architecture.X64
            ? LstatInode64(path, buffer)
            : Lstat(path, buffer);

    private static uint GetEffectiveUserId() =>
        OperatingSystem.IsLinux() ? LinuxGetEffectiveUserId() : MacGetEffectiveUserId();

    // glibc: libc.so is a linker script on most distributions, the shared object is libc.so.6.
    [DllImport("libc.so.6", EntryPoint = "statx", SetLastError = true)]
    private static extern int Statx(
        int directory,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        int flags,
        uint mask,
        [Out] byte[] buffer
    );

    [DllImport("libc", EntryPoint = "lstat", SetLastError = true)]
    private static extern int Lstat(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        [Out] byte[] buffer
    );

    [DllImport("libc", EntryPoint = "lstat$INODE64", SetLastError = true)]
    private static extern int LstatInode64(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        [Out] byte[] buffer
    );

    [DllImport("libc.so.6", EntryPoint = "geteuid")]
    private static extern uint LinuxGetEffectiveUserId();

    [DllImport("libc", EntryPoint = "geteuid")]
    private static extern uint MacGetEffectiveUserId();
}
