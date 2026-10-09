using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Sermofur.Daemon;

/// <summary>
/// Local journal of the daemon (FR-020, research R9): one JSON object per line, local time with
/// its offset, three files of 1 MiB at most. Only events, codes, client process IDs and
/// durations are written; never arguments, outputs or paths given by a client.
/// <para>
/// A caller never waits for the file: <see cref="Write"/> queues the line and returns. One
/// thread per journal object writes the queued lines in batches. Several processes share the
/// file (daemon, supervisor, MCP bridges): the lock file <c>daemon.log.lock</c> beside it gives
/// each batch, and each rotation, to one process at a time; a batch is bounded, so that the
/// others get their turn during a burst. Lines are lost when the queue is full, when another
/// process holds the turn for a whole second, when the file cannot be written, or when the
/// process ends abruptly before <see cref="FlushBeforeExit"/>.
/// </para>
/// </summary>
public sealed class DaemonLog(
    string file,
    Func<DateTimeOffset>? clock = null,
    long maxFileBytes = DaemonLog.MaxFileBytes
)
{
    public const long MaxFileBytes = 1024 * 1024;
    public const int KeptFiles = 3;

    /// <summary>Lines waiting for the writer, at most; further lines are dropped.</summary>
    public const int MaxPendingLines = 4096;

    /// <summary>Lines written in one turn at most, before the turn goes back to the others.</summary>
    public const int MaxBatchLines = 512;

    /// <summary>
    /// How long a process gives its last lines before it exits: the wait for a turn plus one
    /// batch.
    /// </summary>
    public static readonly TimeSpan ExitFlushTimeout = TimeSpan.FromSeconds(2);

    /// <summary>
    /// Longest the writer waits for its turn; far above the time a batch takes, so that lines are
    /// lost only when another process is stuck while holding the turn.
    /// </summary>
    private static readonly TimeSpan TurnTimeout = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan TurnPoll = TimeSpan.FromMilliseconds(1);

    // A local file read by people: the offset of the local time stays "+02:00", not "+02:00".
    private static readonly JsonSerializerOptions Options = new JsonSerializerOptions
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    private readonly Func<DateTimeOffset> now = clock ?? (() => DateTimeOffset.Now);
    private readonly ConcurrentQueue<byte[]> pending = new();
    private int queued;
    private int writing;

    /// <summary>Queues one line; never blocks on the file.</summary>
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
        if (Interlocked.Increment(ref queued) > MaxPendingLines)
        {
            Interlocked.Decrement(ref queued);
            return;
        }
        pending.Enqueue(Encoding.UTF8.GetBytes(line + "\n"));
        if (Interlocked.CompareExchange(ref writing, 1, 0) == 0)
        {
            // A thread of its own: waiting for the turn must not hold a thread of the pool.
            Thread writer = new Thread(WriteQueued)
            {
                IsBackground = true,
                Name = "sermofur-journal",
            };
            writer.Start();
        }
    }

    /// <summary>
    /// Waits until the queued lines are written, or the timeout passes; true when nothing is
    /// left. Called before a process exits.
    /// </summary>
    public bool Flush(TimeSpan timeout)
    {
        Stopwatch waited = Stopwatch.StartNew();
        while (Volatile.Read(ref queued) > 0 || Volatile.Read(ref writing) != 0)
        {
            if (waited.Elapsed >= timeout)
            {
                return false;
            }
            Thread.Sleep(2);
        }
        return true;
    }

    /// <summary>
    /// Writes the queued lines before the process exits (daemon, supervisor, MCP bridge); the
    /// journal writes in the background, so its last lines would otherwise be lost.
    /// </summary>
    public void FlushBeforeExit() => Flush(ExitFlushTimeout);

    private void WriteQueued()
    {
        while (true)
        {
            try
            {
                WriteBatch();
            }
            catch (Exception)
            {
                // Whatever happens, the journal never ends the process that writes it.
                Drop();
            }
            Volatile.Write(ref writing, 0);
            // A line queued after the batch and before the flag went down started no writer;
            // lines left by a bounded batch are written in the next turn.
            if (pending.IsEmpty || Interlocked.CompareExchange(ref writing, 1, 0) != 0)
            {
                return;
            }
        }
    }

    private void WriteBatch()
    {
        // Lines taken from the queue by this batch: a failure loses this batch only, the lines
        // queued after it wait for the next turn.
        int taken = 0;
        try
        {
            using FileStream? turn = FileTurn.TryTake(file + ".lock", TurnTimeout, TurnPoll);
            if (turn is null)
            {
                DropBatch(taken);
                return;
            }
            FileStream journal = OpenJournal();
            try
            {
                while (taken < MaxBatchLines && pending.TryDequeue(out byte[]? line))
                {
                    taken++;
                    Interlocked.Decrement(ref queued);
                    if (journal.Length >= maxFileBytes)
                    {
                        journal.Dispose();
                        Rotate();
                        journal = OpenJournal();
                    }
                    journal.Write(line);
                }
            }
            finally
            {
                journal.Dispose();
            }
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            // A journal that cannot be written never stops the daemon.
            DropBatch(taken);
        }
    }

    // Shared for writing too: an smf of an older version, which knows nothing of the turn, can
    // still append its own line; under the turn, recent writers stay one at a time.
    private FileStream OpenJournal() =>
        new FileStream(
            file,
            FileMode.Append,
            FileAccess.Write,
            FileShare.ReadWrite | FileShare.Delete
        );

    /// <summary>Drops the rest of a failed batch, leaving the lines queued after it.</summary>
    private void DropBatch(int taken)
    {
        for (int index = taken; index < MaxBatchLines && pending.TryDequeue(out _); index++)
        {
            Interlocked.Decrement(ref queued);
        }
    }

    private void Drop()
    {
        while (pending.TryDequeue(out _))
        {
            Interlocked.Decrement(ref queued);
        }
    }

    private void Rotate()
    {
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
