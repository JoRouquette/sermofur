[English](../mcp-integration.md) | Français

# Intégration MCP — conception, non livrée

Pont `sermofur-mcp` stdio → daemon local partagé. Aucun moteur de mémoire par client.
Noms d'outils prévus : `sermofur_context`, `sermofur_recall`, `sermofur_challenge`, `sermofur_record_retex`,
`sermofur_feedback`, `sermofur_claim`, `sermofur_evidence`, `sermofur_status`.
Schémas des outils et transport restent à implémenter et tester. Ni SQL brut ni accès brut au
système de fichiers. Le contexte est lié à la session par le daemon ; un scope n'est jamais
choisi arbitrairement par des arguments du LLM.

Hosts prévus, dans l'ordre : Claude Code, Codex, puis d'autres clients MCP ; leurs capacités de
transport seront vérifiées au moment de construire le pont. Aucun host configuré et aucun test
protocolaire annoncé en 0.1. Les installateurs seront idempotents et préserveront la
configuration existante.
