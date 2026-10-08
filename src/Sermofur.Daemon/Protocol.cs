using System.Text.Json.Serialization;

namespace Sermofur.Daemon;

/// <summary>Kinds of message of protocol 1 (contracts/ipc.md).</summary>
public static class MessageKind
{
    public const string Hello = "hello";
    public const string Welcome = "welcome";
    public const string Run = "run";
    public const string Result = "result";
    public const string NotServing = "not_serving";
    public const string Status = "status";
    public const string Shutdown = "shutdown";
    public const string Stopping = "stopping";
    public const string Error = "error";
}

/// <summary>
/// One message of the local protocol. A single flat shape keeps the wire format readable and
/// lets an older peer ignore the fields it does not know; <see cref="Kind"/> says which fields
/// are meaningful.
/// </summary>
public sealed record IpcMessage
{
    /// <summary>Version of the protocol spoken by this build.</summary>
    public const int CurrentProtocol = 1;

    [JsonPropertyName("kind")]
    public required string Kind { get; init; }

    [JsonPropertyName("protocol")]
    public int? Protocol { get; init; }

    [JsonPropertyName("toolVersion")]
    public string? ToolVersion { get; init; }

    [JsonPropertyName("cwd")]
    public string? Cwd { get; init; }

    [JsonPropertyName("pid")]
    public int? Pid { get; init; }

    [JsonPropertyName("id")]
    public long? Id { get; init; }

    [JsonPropertyName("argv")]
    public IReadOnlyList<string>? Argv { get; init; }

    [JsonPropertyName("exitCode")]
    public int? ExitCode { get; init; }

    [JsonPropertyName("stdout")]
    public string? Stdout { get; init; }

    [JsonPropertyName("stderr")]
    public string? Stderr { get; init; }

    [JsonPropertyName("code")]
    public string? Code { get; init; }

    [JsonPropertyName("message")]
    public string? Message { get; init; }

    [JsonPropertyName("startedAt")]
    public string? StartedAt { get; init; }

    [JsonPropertyName("instancesOpen")]
    public int? InstancesOpen { get; init; }

    [JsonPropertyName("clients")]
    public int? Clients { get; init; }

    public static IpcMessage Failure(string code, string message, long? id = null) =>
        new IpcMessage
        {
            Kind = MessageKind.Error,
            Code = code,
            Message = message,
            Id = id,
        };
}
