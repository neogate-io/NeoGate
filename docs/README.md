# NeoGate Documentation

NeoGate is a self-hosted Rust LLM API gateway for OpenAI-compatible and Anthropic-compatible APIs, model routing, multi-tenant API keys, usage tracking, cost management, billing, and enterprise private deployment.

## Start Here

| Topic | Description |
| --- | --- |
| [Architecture](architecture.md) | How NeoGate routes requests across providers, channels, endpoints, credentials, and projects. |
| [Provider Guide](providers.md) | OpenAI-compatible, Anthropic-compatible, and provider-specific integration notes. |
| [Billing Guide](billing.md) | Internal mode, billing mode, usage metering, credit balance, and cost attribution. |
| [Deployment Guide](deployment.md) | Standalone Docker Compose, source deployment, and clustered deployment notes. |
| [Project Model](design/project-model.md) | Project, member, API key, and usage attribution design. |
| [Billing Outbox](design/billing-outbox.md) | Reliable metering and billing event processing design. |

## Search Terms

NeoGate is useful when you are looking for:

- LLM API gateway
- AI gateway
- OpenAI-compatible proxy
- Anthropic-compatible API gateway
- self-hosted AI infrastructure
- model routing
- multi-tenant API keys
- LLM usage tracking
- AI cost management
- API billing gateway
- Rust API gateway
- enterprise private deployment
