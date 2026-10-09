using System.Text.Json;
using System.Text.Json.Nodes;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;
using Sermofur.Daemon;
using Sermofur.Domain;

namespace Sermofur.Mcp;

/// <summary>
/// <c>smf mcp serve</c>: the MCP server on stdio. It answers initialize and the list of tools
/// without a daemon; every call goes to the daemon through the channel (FR-005, FR-010). Nothing
/// else is ever written to stdout (FR-011).
/// </summary>
public sealed class McpBridge(ICommandChannel channel, string version, DaemonLog? log = null)
{
    public const string ServerName = "sermofur";

    public McpServerOptions Options() =>
        new McpServerOptions
        {
            ServerInfo = new Implementation { Name = ServerName, Version = version },
            ServerInstructions =
                "Sermofur keeps a contestable memory of this project: recall before deciding, challenge a claim before relying on it, and record what you learn as claims, evidence or RETEX. What you write is recorded as coming from an LLM.",
            Handlers = new McpServerHandlers
            {
                ListToolsHandler = (_, _) => ValueTask.FromResult(ListTools()),
                CallToolHandler = async (context, cancellation) =>
                    await CallAsync(
                        context.Params?.Name ?? "",
                        context.Params?.Arguments is { } given
                            ? new Dictionary<string, JsonElement>(given)
                            : null,
                        Session.Actor(context.Server.ClientInfo?.Name),
                        cancellation
                    ),
            },
        };

    public async Task RunAsync(CancellationToken stop)
    {
        McpServerOptions options = Options();
        await using StdioServerTransport transport = new StdioServerTransport(options);
        await using McpServer server = McpServer.Create(transport, options);
        log?.Write("mcp_started");
        try
        {
            await server.RunAsync(stop);
            log?.Write("mcp_stopped");
        }
        finally
        {
            log?.FlushBeforeExit();
        }
    }

    public static ListToolsResult ListTools() =>
        new ListToolsResult
        {
            Tools =
            [
                .. Tools.All.Select(tool => new Tool
                {
                    Name = tool.Name,
                    Description = tool.Description,
                    InputSchema = tool.InputSchema(),
                    // Hosts may skip approval for read-only tools (Codex: approval mode "writes").
                    Annotations = new ToolAnnotations
                    {
                        ReadOnlyHint = tool.ReadOnly,
                        DestructiveHint = false,
                        OpenWorldHint = false,
                    },
                }),
            ],
        };

    /// <summary>One call: validation, execution through the daemon, result or error result.</summary>
    public async ValueTask<CallToolResult> CallAsync(
        string name,
        IReadOnlyDictionary<string, JsonElement>? arguments,
        string actor,
        CancellationToken cancellation
    )
    {
        ToolDefinition? tool = Tools.All.FirstOrDefault(candidate => candidate.Name == name);
        try
        {
            if (tool is null)
            {
                throw new SermofurException("invalid_input", $"Unknown tool '{name}'.");
            }
            ToolArguments validated = ToolArguments.Validate(tool.Fields, arguments);
            JsonNode data = await tool.Execute(
                new ToolCall(validated, actor, channel),
                cancellation
            );
            log?.Write("mcp_call");
            JsonObject structured = data as JsonObject ?? new JsonObject { ["items"] = data };
            return new CallToolResult
            {
                Content = [new TextContentBlock { Text = data.ToJsonString() }],
                StructuredContent = JsonSerializer.SerializeToElement(structured),
            };
        }
        catch (SermofurException exception)
        {
            log?.Write("mcp_call", exception.Code);
            return new CallToolResult
            {
                IsError = true,
                Content =
                [
                    new TextContentBlock { Text = $"{exception.Code}: {exception.Message}" },
                ],
                StructuredContent = JsonSerializer.SerializeToElement(
                    new JsonObject { ["code"] = exception.Code, ["message"] = exception.Message }
                ),
            };
        }
    }
}
