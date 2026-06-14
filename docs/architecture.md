# Architecture

NeoGate provides one controlled entry point for model API traffic. The backend is written in Rust and focuses on request forwarding, credential isolation, model routing, usage recording, and billing-safe metering.

## Core Concepts

| Concept | Role |
| --- | --- |
| Provider | A model vendor or protocol family, such as OpenAI-compatible or Anthropic-compatible APIs. |
| Channel | A managed upstream service configuration owned by the gateway administrator. |
| Endpoint | A concrete upstream base URL and protocol attached to a channel. |
| Credential | An upstream API key or runtime credential used by the relay layer. |
| Project | A business app, team, customer, or cost center with members, API keys, budgets, and usage attribution. |
| Project API key | The key issued to applications that call NeoGate. |
| Usage record | A metering record used for troubleshooting, cost analysis, chargeback, and billing. |

## Request Flow

1. A client calls NeoGate through an OpenAI-compatible or Anthropic-compatible endpoint.
2. NeoGate authenticates the project API key and checks project policy, service mode, and balance requirements.
3. The relay layer selects an enabled upstream channel, endpoint, and credential based on model, priority, weight, and health.
4. The request is forwarded to the upstream provider.
5. Usage is parsed from the response and recorded by user, project, API key, model, provider, and channel.
6. In billing mode, metered usage updates project and account balances through the billing pipeline.

## Deployment Shape

Standalone deployment starts frontend Nginx, backend, and PostgreSQL with Docker Compose. Clustered deployment runs multiple backend API and worker replicas against shared PostgreSQL and Redis.

