using System.Text.Encodings.Web;
using System.Text.Json;

namespace Sermofur.Daemon;

/// <summary>
/// Local journal of the daemon (FR-020, research R9): one JSON object per line, local time with
/// its offset, three files of 1 MiB at most. Only events, codes, client process IDs and
/// durations are written; never arguments, outputs or paths given by a client.
/// </summary>
public sealed class DaemonLog(string file, Func<DateTimeOffset>? clock = null)
{
    public const long MaxFileBytes = 1024 * 1024;
    public const int KeptFiles = 3;

    // A local file read by people: the offset of the local time stays "+02:00", not "+02:00".
    private static readonly JsonSerializerOptions Options = new JsonSerializerOptions
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    private readonly object gate = new();
    private readonly Func<DateTimeOffset> now = clock ?? (() => DateTimeOffset.Now);

    public void Write(
        string evt,
        string? code = null,
        int? clientPid = null,
        long? durationMs = null
    )
    {
        string line = JsonSerializer.Serialize(
            new Dictionary<string, object?>
            {
                ["time"] = now().ToString("yyyy-MM-dd'T'HH:mm:ss.fffzzz"),
                ["event"] = evt,
                ["code"] = code,
                ["pid"] = clientPid,
                ["durationMs"] = durationMs,
            }
                .Where(pair => pair.Value is not null)
                .ToDictionary(pair => pair.Key, pair => pair.Value),
            Options
        );
        lock (gate)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(file)!);
                Rotate();
                File.AppendAllText(file, line + "\n");
            }
            catch (IOException)
            {
                // A journal that cannot be written never stops the daemon.
            }
            catch (UnauthorizedAccessException) { }
        }
    }

    private void Rotate()
    {
        FileInfo current = new FileInfo(file);
        if (!current.Exists || current.Length < MaxFileBytes)
        {
            return;
        }
        for (int index = KeptFiles - 1; index >= 1; index--)
        {
            string older = $"{file}.{index}";
            string source = index == 1 ? file : $"{file}.{index - 1}";
            if (File.Exists(source))
            {
                File.Move(source, older, overwrite: true);
            }
        }
    }
}
