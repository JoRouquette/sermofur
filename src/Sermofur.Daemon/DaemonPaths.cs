using System.Runtime.Versioning;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using Sermofur.Domain;

namespace Sermofur.Daemon;

/// <summary>
/// Where the daemon of the current user keeps its registry, its state and its endpoint
/// (data-model, research R8 and R9). <c>SERMOFUR_DAEMON_HOME</c> moves all three under one
/// folder, so that tests and trials never reach the real daemon of the user.
/// </summary>
public sealed record DaemonPaths(string ConfigDirectory, string StateDirectory, string Endpoint)
{
    /// <summary>Environment variable that replaces every location.</summary>
    public const string HomeVariable = "SERMOFUR_DAEMON_HOME";

    /// <summary>
    /// Longest Unix socket path, in bytes, accepted on every supported system: sun_path holds
    /// 104 bytes on macOS and 108 on Linux, terminator included.
    /// </summary>
    public const int MaxSocketPathBytes = 103;

    public string RegistryFile => Path.Combine(ConfigDirectory, "instances.json");

    public string LogFile => Path.Combine(StateDirectory, "daemon.log");

    public string LockFile => Path.Combine(StateDirectory, "daemon.lock");

    /// <summary>Folder that holds the Unix socket; null on Windows, where the endpoint is a pipe.</summary>
    public string? RuntimeDirectory =>
        OperatingSystem.IsWindows() ? null : Path.GetDirectoryName(Endpoint);

    /// <summary>Locations of the current user, from the environment of this process.</summary>
    public static DaemonPaths ForCurrentUser() =>
        Resolve(Environment.GetEnvironmentVariable, Environment.GetFolderPath);

    /// <summary>Resolution with replaceable environment lookups, for tests.</summary>
    internal static DaemonPaths Resolve(
        Func<string, string?> variable,
        Func<Environment.SpecialFolder, string> folder
    )
    {
        string? home = NonEmpty(variable(HomeVariable));
        if (home is not null)
        {
            string root = Path.GetFullPath(home);
            string endpoint = OperatingSystem.IsWindows()
                ? PipeName("home-" + Hash(root))
                : Path.Combine(root, "run", "daemon.sock");
            return Checked(
                new DaemonPaths(Path.Combine(root, "config"), Path.Combine(root, "state"), endpoint)
            );
        }
        if (OperatingSystem.IsWindows())
        {
            return new DaemonPaths(
                Path.Combine(folder(Environment.SpecialFolder.ApplicationData), "Sermofur"),
                Path.Combine(folder(Environment.SpecialFolder.LocalApplicationData), "Sermofur"),
                PipeName(Hash(CurrentWindowsSid()))
            );
        }
        string userHome = folder(Environment.SpecialFolder.UserProfile);
        uint uid = UnixNative.EffectiveUserId();
        string temporary = NonEmpty(variable("TMPDIR")) ?? "/tmp";
        if (OperatingSystem.IsMacOS())
        {
            return Checked(
                new DaemonPaths(
                    Path.Combine(userHome, "Library", "Application Support", "Sermofur"),
                    Path.Combine(userHome, "Library", "Logs", "Sermofur"),
                    Path.Combine(temporary, $"sermofur-{uid}", "daemon.sock")
                )
            );
        }
        string config = NonEmpty(variable("XDG_CONFIG_HOME")) ?? Path.Combine(userHome, ".config");
        string state =
            NonEmpty(variable("XDG_STATE_HOME")) ?? Path.Combine(userHome, ".local", "state");
        string? runtime = NonEmpty(variable("XDG_RUNTIME_DIR"));
        string socketFolder = runtime is not null
            ? Path.Combine(runtime, "sermofur")
            : Path.Combine(temporary, $"sermofur-{uid}");
        return Checked(
            new DaemonPaths(
                Path.Combine(config, "sermofur"),
                Path.Combine(state, "sermofur"),
                Path.Combine(socketFolder, "daemon.sock")
            )
        );
    }

    private static DaemonPaths Checked(DaemonPaths paths)
    {
        if (
            !OperatingSystem.IsWindows()
            && Encoding.UTF8.GetByteCount(paths.Endpoint) > MaxSocketPathBytes
        )
        {
            throw new SermofurException(
                "invalid_path",
                $"The daemon socket path is longer than {MaxSocketPathBytes} bytes: {paths.Endpoint}. Set {HomeVariable} or TMPDIR to a shorter folder.",
                3
            );
        }
        return paths;
    }

    private static string PipeName(string suffix) => $"sermofur-{suffix}";

    private static string Hash(string value) =>
        Convert
            .ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(value)))[..16]
            .ToLowerInvariant();

    [SupportedOSPlatform("windows")]
    private static string CurrentWindowsSid()
    {
        using WindowsIdentity identity = WindowsIdentity.GetCurrent();
        return identity.User?.Value
            ?? throw new SermofurException(
                "invalid_storage_or_path",
                "The current Windows account has no SID.",
                3
            );
    }

    private static string? NonEmpty(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value;
}
