// Records what AtomicFile.Write of src/Sermofur.Infrastructure does on crafted folders: plain
// files, links and junctions in place of the target, chains, circles, dead ends and folders, with
// both link policies. The scratch root is replaced by "<root>" in paths and messages, and "\"
// by "/". Usage: dotnet run atomic-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;
using Sermofur.Infrastructure;

string scratch = Path.Combine(
    Path.GetTempPath(),
    "sermofur-atomic-oracle-" + Guid.NewGuid().ToString("N")
);

void Junction(string link, string target)
{
    if (OperatingSystem.IsWindows())
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
    else
    {
        Directory.CreateSymbolicLink(link, target);
    }
}

// Each scenario prepares a folder, then writes "new" to the file it names with a policy.
List<(string Name, LinkPolicy Policy, string File, Action<string> Prepare)> scenarios =
[
    ("new_file", LinkPolicy.Refuse, "a.txt", _ => { }),
    (
        "replace_existing",
        LinkPolicy.Refuse,
        "a.txt",
        root => File.WriteAllText(Path.Combine(root, "a.txt"), "old")
    ),
    ("missing_parent_folders", LinkPolicy.Refuse, "sub/dir/a.txt", _ => { }),
    (
        "refuse_symlink_to_file",
        LinkPolicy.Refuse,
        "link",
        root =>
        {
            File.WriteAllText(Path.Combine(root, "target"), "old");
            File.CreateSymbolicLink(Path.Combine(root, "link"), Path.Combine(root, "target"));
        }
    ),
    (
        "refuse_dangling_symlink",
        LinkPolicy.Refuse,
        "link",
        root => File.CreateSymbolicLink(Path.Combine(root, "link"), Path.Combine(root, "nowhere"))
    ),
    (
        "refuse_folder",
        LinkPolicy.Refuse,
        "folder",
        root => Directory.CreateDirectory(Path.Combine(root, "folder"))
    ),
    (
        "refuse_junction",
        LinkPolicy.Refuse,
        "junction",
        root =>
        {
            Directory.CreateDirectory(Path.Combine(root, "dir"));
            Junction(Path.Combine(root, "junction"), Path.Combine(root, "dir"));
        }
    ),
    (
        "follow_no_link",
        LinkPolicy.FollowToTarget,
        "a.txt",
        root => File.WriteAllText(Path.Combine(root, "a.txt"), "old")
    ),
    ("follow_new_file", LinkPolicy.FollowToTarget, "a.txt", _ => { }),
    (
        "follow_one_link",
        LinkPolicy.FollowToTarget,
        "link",
        root =>
        {
            File.WriteAllText(Path.Combine(root, "target"), "old");
            File.CreateSymbolicLink(Path.Combine(root, "link"), Path.Combine(root, "target"));
        }
    ),
    (
        "follow_relative_link",
        LinkPolicy.FollowToTarget,
        "link",
        root =>
        {
            Directory.CreateDirectory(Path.Combine(root, "sub"));
            File.WriteAllText(Path.Combine(root, "sub", "target"), "old");
            File.CreateSymbolicLink(Path.Combine(root, "link"), Path.Combine("sub", "target"));
        }
    ),
    (
        "follow_relative_dot_dot",
        LinkPolicy.FollowToTarget,
        "sub/link",
        root =>
        {
            Directory.CreateDirectory(Path.Combine(root, "sub"));
            File.WriteAllText(Path.Combine(root, "target"), "old");
            File.CreateSymbolicLink(
                Path.Combine(root, "sub", "link"),
                Path.Combine("..", "target")
            );
        }
    ),
    ("follow_chain_10", LinkPolicy.FollowToTarget, "l0", root => Chain(root, 10)),
    ("follow_chain_11", LinkPolicy.FollowToTarget, "l0", root => Chain(root, 11)),
    (
        "follow_circle",
        LinkPolicy.FollowToTarget,
        "a",
        root =>
        {
            File.CreateSymbolicLink(Path.Combine(root, "a"), Path.Combine(root, "b"));
            File.CreateSymbolicLink(Path.Combine(root, "b"), Path.Combine(root, "a"));
        }
    ),
    (
        "follow_self",
        LinkPolicy.FollowToTarget,
        "a",
        root => File.CreateSymbolicLink(Path.Combine(root, "a"), Path.Combine(root, "a"))
    ),
    (
        "follow_dangling",
        LinkPolicy.FollowToTarget,
        "link",
        root => File.CreateSymbolicLink(Path.Combine(root, "link"), Path.Combine(root, "nowhere"))
    ),
    (
        "follow_to_folder",
        LinkPolicy.FollowToTarget,
        "link",
        root =>
        {
            Directory.CreateDirectory(Path.Combine(root, "dir"));
            Directory.CreateSymbolicLink(Path.Combine(root, "link"), Path.Combine(root, "dir"));
        }
    ),
    (
        "follow_folder_itself",
        LinkPolicy.FollowToTarget,
        "dir",
        root => Directory.CreateDirectory(Path.Combine(root, "dir"))
    ),
    (
        "follow_junction",
        LinkPolicy.FollowToTarget,
        "junction",
        root =>
        {
            Directory.CreateDirectory(Path.Combine(root, "dir"));
            Junction(Path.Combine(root, "junction"), Path.Combine(root, "dir"));
        }
    ),
    (
        "follow_link_to_link_to_file",
        LinkPolicy.FollowToTarget,
        "l1",
        root =>
        {
            File.WriteAllText(Path.Combine(root, "target"), "old");
            File.CreateSymbolicLink(Path.Combine(root, "l2"), Path.Combine(root, "target"));
            File.CreateSymbolicLink(Path.Combine(root, "l1"), Path.Combine(root, "l2"));
        }
    ),
    (
        "follow_case_variant_circle",
        LinkPolicy.FollowToTarget,
        "a",
        root =>
        {
            File.CreateSymbolicLink(Path.Combine(root, "a"), Path.Combine(root, "b"));
            File.CreateSymbolicLink(Path.Combine(root, "b"), Path.Combine(root, "A"));
        }
    ),
];

static void Chain(string root, int links)
{
    File.WriteAllText(Path.Combine(root, "target"), "old");
    for (int i = links - 1; i >= 0; i--)
    {
        string next = i == links - 1 ? "target" : $"l{i + 1}";
        File.CreateSymbolicLink(Path.Combine(root, $"l{i}"), Path.Combine(root, next));
    }
}

string Show(string root, string text) => text.Replace(root, "<root>").Replace('\\', '/');

JsonArray rows = [];
foreach ((string name, LinkPolicy policy, string file, Action<string> prepare) in scenarios)
{
    string root = Path.Combine(scratch, name);
    Directory.CreateDirectory(root);
    prepare(root);
    JsonObject row = new()
    {
        ["name"] = name,
        ["policy"] = policy == LinkPolicy.Refuse ? "refuse" : "follow",
        ["file"] = file,
    };
    try
    {
        string written = AtomicFile.Write(
            Path.Combine(root, file),
            Encoding.UTF8.GetBytes("new"),
            new WriteOptions(Policy: policy)
        );
        row["written"] = Show(root, written);
    }
    catch (SermofurException exception)
    {
        row["code"] = exception.Code;
        row["message"] = Show(root, exception.Message);
        row["exitCode"] = exception.ExitCode;
    }
    // What the folder holds afterwards: every entry, its kind and the content of plain files.
    JsonObject entries = [];
    foreach (
        string entry in Directory
            .EnumerateFileSystemEntries(root, "*", SearchOption.AllDirectories)
            .Order(StringComparer.Ordinal)
    )
    {
        FileSystemInfo info = Directory.Exists(entry)
            ? new DirectoryInfo(entry)
            : new FileInfo(entry);
        string relative = Path.GetRelativePath(root, entry).Replace('\\', '/');
        entries[relative] =
            info.LinkTarget is not null ? "link"
            : info is DirectoryInfo ? "folder"
            : File.ReadAllText(entry);
    }
    row["after"] = entries;
    rows.Add(row);
}

// Junctions and links must not be followed by the recursive delete.
foreach (
    string entry in Directory
        .EnumerateFileSystemEntries(scratch, "*", SearchOption.AllDirectories)
        .ToList()
)
{
    if (
        new FileInfo(entry).LinkTarget is not null
        || new DirectoryInfo(entry).LinkTarget is not null
    )
    {
        if (Directory.Exists(entry))
        {
            Directory.Delete(entry);
        }
        else
        {
            File.Delete(entry);
        }
    }
}
Directory.Delete(scratch, recursive: true);

JsonObject result = new()
{
    ["generator"] = "rust/tools/atomic-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["os"] = OperatingSystem.IsWindows() ? "windows" : "unix",
    ["scenarios"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} scenarios");
