English | [Français](fr/mcp-integration.md)

# MCP integration — design, not delivered

A `sermofur-mcp` stdio bridge → shared local daemon. No memory engine per client.
Planned tool names: `sermofur_context`, `sermofur_recall`, `sermofur_challenge`, `sermofur_record_retex`,
`sermofur_feedback`, `sermofur_claim`, `sermofur_evidence`, `sermofur_status`.
Tool schemas and transport are still to be implemented and tested. No raw SQL nor raw file
system access. The context is bound to the session by the daemon; a scope is never chosen
arbitrarily through LLM arguments.

Planned hosts, in order: Claude Code, Codex, then other MCP clients; their transport capabilities
will be checked when the bridge is built. No host is configured and no protocol test is claimed
in 0.1. Installers will be idempotent and preserve existing configuration.
