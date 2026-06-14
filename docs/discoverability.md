# Discoverability Guide

This guide collects the repository metadata and positioning text that help people find NeoGate through GitHub search, topic pages, and search engines.

## GitHub About

Recommended repository description:

```text
Self-hosted Rust LLM API gateway with OpenAI-compatible and Anthropic-compatible APIs, model routing, multi-tenant keys, usage tracking, billing, and admin console.
```

Recommended website field:

```text
https://github.com/neogate-io/NeoGate
```

## GitHub Topics

Recommended topics:

```text
llm-gateway
ai-gateway
api-gateway
openai-compatible
anthropic-api
openai-api
self-hosted
multi-tenant
model-routing
usage-tracking
cost-management
billing
rust
docker-compose
ai-infrastructure
llm-observability
```

## Social Preview

Use this image in GitHub repository settings:

```text
assets/social-preview.png
```

GitHub path:

```text
Settings -> Social preview -> Edit
```

## GitHub CLI Commands

After authenticating with `gh auth login`, update repository metadata with:

```bash
gh repo edit neogate-io/NeoGate \
  --description "Self-hosted Rust LLM API gateway with OpenAI-compatible and Anthropic-compatible APIs, model routing, multi-tenant keys, usage tracking, billing, and admin console." \
  --homepage "https://github.com/neogate-io/NeoGate" \
  --add-topic llm-gateway \
  --add-topic ai-gateway \
  --add-topic api-gateway \
  --add-topic openai-compatible \
  --add-topic anthropic-api \
  --add-topic openai-api \
  --add-topic self-hosted \
  --add-topic multi-tenant \
  --add-topic model-routing \
  --add-topic usage-tracking \
  --add-topic cost-management \
  --add-topic billing \
  --add-topic rust \
  --add-topic docker-compose \
  --add-topic ai-infrastructure \
  --add-topic llm-observability
```

GitHub CLI does not currently upload social preview images through `gh repo edit`; upload `assets/social-preview.png` from the repository settings page.

