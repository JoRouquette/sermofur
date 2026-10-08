using System.Buffers.Binary;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Sermofur.Domain;

namespace Sermofur.Daemon;

/// <summary>
/// Length-prefixed frames: a little-endian <c>uint32</c> with the size of the body, then a UTF-8
/// JSON body without BOM, at most <see cref="MaxBodyBytes"/> bytes (research R4).
/// </summary>
public static class Framing
{
    /// <summary>Largest body accepted: covers 128 arguments of 16 384 characters with escaping.</summary>
    public const int MaxBodyBytes = 256 * 1024;

    private static readonly JsonSerializerOptions Options = new JsonSerializerOptions
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private static readonly UTF8Encoding StrictUtf8 = new UTF8Encoding(false, true);

    /// <summary>
    /// Reads one message, or null when the peer closed the stream cleanly between two frames.
    /// A truncated, oversized or malformed frame is a <see cref="SermofurException"/>.
    /// </summary>
    public static async Task<IpcMessage?> ReadAsync(Stream stream, CancellationToken cancellation)
    {
        byte[] header = new byte[4];
        int headerRead = await FillAsync(stream, header, cancellation);
        if (headerRead == 0)
        {
            return null;
        }
        if (headerRead < header.Length)
        {
            throw ProtocolError("Truncated frame header.");
        }
        uint length = BinaryPrimitives.ReadUInt32LittleEndian(header);
        if (length == 0)
        {
            throw ProtocolError("Empty frame.");
        }
        if (length > MaxBodyBytes)
        {
            throw new SermofurException(
                "request_too_large",
                $"Frame of {length} bytes; the limit is {MaxBodyBytes}.",
                1
            );
        }
        byte[] body = new byte[length];
        if (await FillAsync(stream, body, cancellation) < body.Length)
        {
            throw ProtocolError("Truncated frame body.");
        }
        return Decode(body);
    }

    /// <summary>Writes one message as a single frame and flushes it.</summary>
    public static async Task WriteAsync(
        Stream stream,
        IpcMessage message,
        CancellationToken cancellation
    )
    {
        byte[] frame = Encode(message);
        await stream.WriteAsync(frame, cancellation);
        await stream.FlushAsync(cancellation);
    }

    /// <summary>Header and body of a message; refuses a body over the limit.</summary>
    public static byte[] Encode(IpcMessage message)
    {
        byte[] body = JsonSerializer.SerializeToUtf8Bytes(message, Options);
        if (body.Length > MaxBodyBytes)
        {
            throw new SermofurException(
                "request_too_large",
                $"Message of {body.Length} bytes; the limit is {MaxBodyBytes}.",
                1
            );
        }
        byte[] frame = new byte[body.Length + 4];
        BinaryPrimitives.WriteUInt32LittleEndian(frame, (uint)body.Length);
        body.CopyTo(frame, 4);
        return frame;
    }

    /// <summary>Message of a frame body; malformed UTF-8 or JSON, or no kind, is a protocol error.</summary>
    public static IpcMessage Decode(byte[] body)
    {
        try
        {
            // Strict decoding first: invalid UTF-8 must not be repaired into a valid request.
            string text = StrictUtf8.GetString(body);
            IpcMessage? message = JsonSerializer.Deserialize<IpcMessage>(text, Options);
            if (message is null || string.IsNullOrEmpty(message.Kind))
            {
                throw ProtocolError("Message without kind.");
            }
            return message;
        }
        catch (Exception exception) when (exception is JsonException or DecoderFallbackException)
        {
            throw ProtocolError("Malformed message.");
        }
    }

    private static async Task<int> FillAsync(
        Stream stream,
        byte[] buffer,
        CancellationToken cancellation
    )
    {
        int total = 0;
        while (total < buffer.Length)
        {
            int read = await stream.ReadAsync(buffer.AsMemory(total), cancellation);
            if (read == 0)
            {
                break;
            }
            total += read;
        }
        return total;
    }

    private static SermofurException ProtocolError(string message) =>
        new SermofurException("protocol_error", message, 3);
}
