using System.Buffers.Binary;
using System.Text;
using FsCheck;
using FsCheck.Fluent;
using FsCheck.Xunit;
using Sermofur.Daemon;
using Sermofur.Domain;

namespace Sermofur.Tests;

public class FramingTests
{
    [Fact]
    public async Task MessageSurvivesARoundTrip()
    {
        IpcMessage sent = new IpcMessage
        {
            Kind = MessageKind.Run,
            Id = 7,
            Argv = ["claim", "add", "é -- \"quoted\"", ""],
            Cwd = Path.GetTempPath(),
        };
        using MemoryStream stream = new MemoryStream();
        await Framing.WriteAsync(stream, sent, CancellationToken.None);
        stream.Position = 0;
        IpcMessage received = (await Framing.ReadAsync(stream, CancellationToken.None))!;
        Assert.Equal(sent.Kind, received.Kind);
        Assert.Equal(sent.Id, received.Id);
        Assert.Equal(sent.Argv, received.Argv);
        Assert.Equal(sent.Cwd, received.Cwd);
        Assert.Null(await Framing.ReadAsync(stream, CancellationToken.None));
    }

    [Fact]
    public void NullFieldsAreNotWritten()
    {
        byte[] frame = Framing.Encode(new IpcMessage { Kind = MessageKind.Status });
        Assert.Equal("{\"kind\":\"status\"}", Encoding.UTF8.GetString(frame, 4, frame.Length - 4));
    }

    [Fact]
    public async Task BodyAtTheLimitIsAcceptedAndOneMoreByteIsRefused()
    {
        string padding = new string('a', Framing.MaxBodyBytes - "{\"kind\":\"\"}".Length);
        byte[] atLimit = Framing.Encode(new IpcMessage { Kind = padding });
        Assert.Equal(Framing.MaxBodyBytes + 4, atLimit.Length);
        Assert.Equal(padding, (await Read(atLimit)).Kind);
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            Framing.Encode(new IpcMessage { Kind = padding + "a" })
        );
        Assert.Equal(("request_too_large", 1), (refusal.Code, refusal.ExitCode));
    }

    [Fact]
    public async Task OversizedHeaderIsRefusedWithoutReadingTheBody()
    {
        byte[] header = new byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(header, Framing.MaxBodyBytes + 1);
        SermofurException refusal = await Assert.ThrowsAsync<SermofurException>(() => Read(header));
        Assert.Equal("request_too_large", refusal.Code);
    }

    [Theory]
    [InlineData(new byte[] { 0, 0, 0, 0 }, "Empty frame.")]
    [InlineData(new byte[] { 5, 0 }, "Truncated frame header.")]
    [InlineData(new byte[] { 5, 0, 0, 0, (byte)'{' }, "Truncated frame body.")]
    public async Task BrokenFramesAreProtocolErrors(byte[] frame, string message)
    {
        SermofurException refusal = await Assert.ThrowsAsync<SermofurException>(() => Read(frame));
        Assert.Equal(
            ("protocol_error", 3, message),
            (refusal.Code, refusal.ExitCode, refusal.Message)
        );
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("{}")]
    [InlineData("{\"kind\":\"\"}")]
    [InlineData("{\"kind\":3}")]
    [InlineData("[]")]
    public void MalformedBodiesAreProtocolErrors(string body)
    {
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            Framing.Decode(Encoding.UTF8.GetBytes(body))
        );
        Assert.Equal("protocol_error", refusal.Code);
    }

    [Fact]
    public void InvalidUtf8IsNotRepaired()
    {
        byte[] body = [.. Encoding.UTF8.GetBytes("{\"kind\":\""), 0xC3, 0x28, .. "\"}"u8];
        Assert.Equal(
            "protocol_error",
            Assert.Throws<SermofurException>(() => Framing.Decode(body)).Code
        );
    }

    [Fact]
    public void UnknownFieldsAreIgnored()
    {
        IpcMessage message = Framing.Decode(
            Encoding.UTF8.GetBytes("{\"kind\":\"status\",\"future\":{\"x\":1}}")
        );
        Assert.Equal(MessageKind.Status, message.Kind);
    }

    /// <summary>
    /// Any well-formed argument vector within the limit comes back unchanged. A lone surrogate
    /// cannot be encoded in UTF-8 at all, so it is outside the property.
    /// </summary>
    [Property(MaxTest = 300)]
    public Property ArgumentsRoundTrip(NonNull<string>[] arguments)
    {
        string[] argv = arguments.Select(a => a.Get).ToArray();
        if (!argv.All(WellFormed))
        {
            return true.ToProperty();
        }
        IpcMessage message = new IpcMessage { Kind = MessageKind.Run, Argv = argv };
        byte[] frame;
        try
        {
            frame = Framing.Encode(message);
        }
        catch (SermofurException exception) when (exception.Code == "request_too_large")
        {
            return true.ToProperty();
        }
        IpcMessage back = Framing.Decode(frame[4..]);
        return (back.Argv ?? []).SequenceEqual(argv).ToProperty();
    }

    /// <summary>Arbitrary bytes never escape as anything but a message or a known refusal.</summary>
    [Property(MaxTest = 500)]
    public Property ArbitraryBytesAreAMessageOrARefusal(byte[] bytes)
    {
        try
        {
            Read(bytes).GetAwaiter().GetResult();
            return true.ToProperty();
        }
        catch (SermofurException exception)
        {
            return (exception.Code is "protocol_error" or "request_too_large").ToProperty();
        }
    }

    private static bool WellFormed(string text)
    {
        try
        {
            new UTF8Encoding(false, true).GetByteCount(text);
            return true;
        }
        catch (EncoderFallbackException)
        {
            return false;
        }
    }

    private static async Task<IpcMessage> Read(byte[] frame)
    {
        using MemoryStream stream = new MemoryStream(frame);
        return (await Framing.ReadAsync(stream, CancellationToken.None))!;
    }
}
