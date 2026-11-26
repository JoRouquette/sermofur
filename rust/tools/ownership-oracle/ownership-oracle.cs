// Records what FileOwnership.Inspect of src/Sermofur.Infrastructure returns on crafted entries:
// folders, files, absent paths, links, junctions, a folder whose owner may not read its security
// descriptor, and system entries of another account. The scratch root is replaced by "<root>"
// and "\" by "/". Usage: dotnet run ownership-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Diagnostics;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Infrastructure;

string scratch = Path.Combine(
    Path.GetTempPath(),
    "sermofur-ownership-oracle-" + Guid.NewGuid().ToString("N")
);
Directory.CreateDirectory(scratch);
string Under(string relative) => Path.Combine(scratch, relative);

void Junction(string link, string target)
{
    using Process process = Process.Start(
        new ProcessStartInfo("cmd.exe", ["/C", "mklink", "/J", link, target])
        {
            RedirectStandardOutput = true,
            UseShellExecute = false,
        }
    )!;
    process.WaitForExit();
}

if (OperatingSystem.IsWindows())
{
    // The owner keeps full control but is denied the right to read the security descriptor.
    SecurityIdentifier ownerRights = new SecurityIdentifier("S-1-3-4");
    SecurityIdentifier user = WindowsIdentity.GetCurrent().User!;
    Directory.CreateDirectory(Under("dir"));
    File.WriteAllText(Under("file.txt"), "x");
    File.CreateSymbolicLink(Under("link-file"), Under("file.txt"));
    Directory.CreateSymbolicLink(Under("link-dir"), Under("dir"));
    File.CreateSymbolicLink(Under("link-dangling"), Under("nowhere"));
    Junction(Under("junction"), Under("dir"));
    Directory.CreateDirectory(Under("no-read-control"));
    DirectorySecurity denied = new DirectorySecurity();
    denied.SetAccessRuleProtection(true, false);
    denied.AddAccessRule(
        new FileSystemAccessRule(user, FileSystemRights.FullControl, AccessControlType.Allow)
    );
    denied.AddAccessRule(
        new FileSystemAccessRule(
            ownerRights,
            FileSystemRights.ReadPermissions,
            AccessControlType.Deny
        )
    );
    new DirectoryInfo(Under("no-read-control")).SetAccessControl(denied);
}
else
{
    Directory.CreateDirectory(Under("dir"));
    File.WriteAllText(Under("file.txt"), "x");
    File.CreateSymbolicLink(Under("link-file"), Under("file.txt"));
    Directory.CreateSymbolicLink(Under("link-dir"), Under("dir"));
    File.CreateSymbolicLink(Under("link-dangling"), Under("nowhere"));
    using Process mkfifo = Process.Start("mkfifo", [Under("fifo")])!;
    mkfifo.WaitForExit();
    // A folder that cannot be searched: its children cannot be read (EACCES), unless as root.
    Directory.CreateDirectory(Under("locked"));
    File.SetUnixFileMode(Under("locked"), UnixFileMode.None);
}

string system = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
List<(string Name, string Path)> cases = OperatingSystem.IsWindows()
    ?
    [
        ("dir", Under("dir")),
        ("dir_trailing_separator", Under("dir") + Path.DirectorySeparatorChar),
        ("file", Under("file.txt")),
        ("file_trailing_dot", Under("file.txt.")),
        ("file_trailing_separator", Under("file.txt") + Path.DirectorySeparatorChar),
        ("absent", Under("absent")),
        ("absent_parent", Under("absent/child")),
        ("through_file", Under("file.txt/child")),
        ("link_to_file", Under("link-file")),
        ("link_to_dir", Under("link-dir")),
        ("link_dangling", Under("link-dangling")),
        ("junction", Under("junction")),
        ("no_read_control", Under("no-read-control")),
        ("system_dir", system),
        ("system_file", Path.Combine(system, "System32", "kernel32.dll")),
    ]
    :
    [
        ("dir", Under("dir")),
        ("dir_trailing_separator", Under("dir") + Path.DirectorySeparatorChar),
        ("file", Under("file.txt")),
        ("file_trailing_separator", Under("file.txt") + Path.DirectorySeparatorChar),
        ("absent", Under("absent")),
        ("absent_parent", Under("absent/child")),
        ("through_file", Under("file.txt/child")),
        ("link_to_file", Under("link-file")),
        ("link_to_dir", Under("link-dir")),
        ("link_dangling", Under("link-dangling")),
        ("fifo", Under("fifo")),
        ("no_search_parent", Under("locked/child")),
        ("system_dir", "/"),
        ("system_file", "/etc/passwd"),
    ];

JsonArray rows = [];
FileOwnership ownership = new FileOwnership();
foreach ((string name, string path) in cases)
{
    JsonObject row = new() { ["name"] = name };
    try
    {
        EntryStatus? status = ownership.Inspect(path);
        row["status"] = status is null
            ? null
            : new JsonObject
            {
                ["isRegularFile"] = status.IsRegularFile,
                ["isDirectory"] = status.IsDirectory,
                ["isOwnedByCurrentUser"] = status.IsOwnedByCurrentUser,
            };
    }
    catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
    {
        // Every caller maps both kinds to "unreadable": only the fact of the failure counts.
        row["error"] = true;
    }
    rows.Add(row);
}

if (OperatingSystem.IsWindows())
{
    // Rewriting the DACL by name needs the right that is denied; deleting the folder does not.
    Directory.Delete(Under("no-read-control"));
    foreach (string link in new[] { "link-file", "link-dangling" })
    {
        File.Delete(Under(link));
    }
    Directory.Delete(Under("link-dir"));
    Directory.Delete(Under("junction"));
}
else
{
    File.SetUnixFileMode(
        Under("locked"),
        UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute
    );
    foreach (string link in new[] { "link-file", "link-dangling", "link-dir" })
    {
        File.Delete(Under(link));
    }
}
Directory.Delete(scratch, recursive: true);

JsonObject result = new()
{
    ["generator"] = "rust/tools/ownership-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["os"] = OperatingSystem.IsWindows() ? "windows" : "unix",
    ["cases"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} cases");
