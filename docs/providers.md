# Provider Guide

NeoGate is designed to manage multiple upstream model providers behind one enterprise-controlled gateway.

## Supported Integration Styles

| Style | Use Case |
| --- | --- |
| OpenAI-compatible API | Existing SDKs and tools that call `/v1/chat/completions`, `/v1/responses`, or related OpenAI-style endpoints. |
| Anthropic-compatible API | Clients that use Anthropic message APIs and Claude-compatible request formats. |
| Provider-specific channels | Admin-managed upstream channels for vendors with custom base URLs, model names, pricing, and credentials. |

## Operational Notes

- Keep upstream credentials in NeoGate instead of distributing vendor keys to every internal app.
- Use project API keys for teams, services, customers, or automation jobs.
- Configure model names and prices before enabling billing mode.
- Use channel priority, weight, and health status to control routing and failover.
- Review usage records by project, API key, model, and provider when troubleshooting cost or latency changes.

## Discovery Keywords

OpenAI-compatible proxy, Anthropic-compatible API gateway, model provider routing, self-hosted LLM gateway, enterprise AI gateway.

