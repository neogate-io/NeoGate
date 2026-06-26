# Deploy NeoGate in 10 Minutes

This guide is for first-time NeoGate evaluators. The goal is to run a self-hosted LLM API gateway on a Docker-ready server or local machine, then complete one real API call through NeoGate.

After finishing this guide, you will have:

- A reachable NeoGate console.
- One configured upstream channel.
- One project or user API key.
- One test request through `/v1/chat/completions`.
- One usage record visible in the admin console.

## When To Use This Guide

This guide is useful when you want to:

- Evaluate NeoGate as an individual developer or small team.
- Set up a first internal AI API entry point for a company or team.
- Quickly validate an upstream provider, model name, and protocol compatibility before production deployment.

If you need multiple replicas, external PostgreSQL, Redis, or production horizontal scaling, see [Cluster Deployment](deployment/cluster.md).

## Prerequisites

You need:

- A machine that can run Docker and Docker Compose.
- A valid upstream model API key, such as an OpenAI-compatible or Anthropic-compatible service.
- Firewall or security group access to the NeoGate port. The default port is `8080`.

Confirm Docker is available:

```bash
docker --version
docker compose version
```

## 1. Start NeoGate

Run this from the NeoGate repository root:

```bash
docker compose up -d
```

This starts:

- `postgres`: PostgreSQL database.
- `backend`: NeoGate backend API, worker, and scheduler.
- `web`: Frontend page and Nginx reverse proxy.

Check runtime status:

```bash
docker compose ps
```

The three services should normally be in `running` or `healthy` state.

If you need logs:

```bash
docker compose logs -f
```

View backend logs only:

```bash
docker compose logs -f backend
```

## 2. Open The Bootstrap Wizard

Open this in your browser:

```text
http://SERVER_IP:8080
```

For local evaluation, use:

```text
http://127.0.0.1:8080
```

On first startup, NeoGate opens the bootstrap wizard. Follow the page to configure:

- Admin account.
- Service mode.
- Site name and public base URL.
- Initial upstream provider, API key, model, and pricing.
- Optional SMTP and payment settings.

For internal evaluation, choose internal mode first. Internal mode does not require users or projects to have a positive balance before requests can be sent, so it is easier for internal gateways, local testing, and small team trials.

## 3. Configure An Upstream Channel

After entering the admin console, open:

```text
Admin Console -> Upstream Channels
```

Make sure at least one channel is enabled, then check:

- The provider is correct.
- The base URL points to your upstream service.
- The API key is valid.
- The model list contains the model you plan to call.
- Channel diagnostics can reach the upstream service successfully.

For OpenAI-compatible services, you usually need an OpenAI protocol endpoint and the correct model name.

For Anthropic-compatible services, make sure the Anthropic protocol endpoint is enabled and uses the matching model name.

## 4. Create A Project And API Key

In internal mode, projects are the recommended place to represent a business app, team, or cost unit:

```text
Admin Console -> Projects
```

Create a project, for example:

```text
Internal AI Gateway
```

Then create or view an API key for a project member. You can also open the user console:

```text
User Console -> API Keys
```

Copy a usable API key. Your application will call NeoGate with this key instead of exposing the upstream provider key.

## 5. Send A Test Request

Replace `YOUR_NEOGATE_API_KEY` and `MODEL_NAME` with your NeoGate API key and model name:

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "MODEL_NAME",
    "messages": [
      {
        "role": "user",
        "content": "Introduce NeoGate in one sentence."
      }
    ]
  }'
```

If NeoGate is deployed on a remote server, replace the address with:

```text
http://SERVER_IP:8080/v1/chat/completions
```

On success, you will receive an OpenAI-compatible response.

## 6. View Usage

After the request finishes, open:

```text
Admin Console -> Usage
```

You should see the request record, including:

- User or project.
- API key.
- Provider and model.
- Token usage.
- Cost.
- Status code and error message.
- Upstream routing path.

You can also open:

```text
Admin Console -> Usage Statistics
```

to view usage and cost aggregated by date, model, channel, and other dimensions.

## Troubleshooting

### The Page Does Not Open

Check container status first:

```bash
docker compose ps
```

Then view logs:

```bash
docker compose logs -f backend
docker compose logs -f web
```

Confirm your firewall, security group, or local port settings do not block `8080`.

### Upstream Requests Fail

Open `Admin Console -> Upstream Channels`, run channel diagnostics, and check:

- The base URL is correct.
- The API key is valid.
- The model name matches the upstream provider.
- The upstream service supports the selected protocol.
- The server can reach the upstream network.

### NeoGate Returns Insufficient Balance

If you selected billing mode, users or projects need available credit before requests can be sent. During evaluation, you can:

- Switch to internal mode.
- Adjust project or user credit in the admin console.
- Configure recharge packages and a payment provider before testing the billing flow.

### Streaming Or Image Requests Timeout

Long-running requests depend on reverse proxy and upstream timeout settings. The default Docker Compose setup includes common streaming and long-request settings. If you use a host-level Nginx proxy, see the Nginx notes in [Standalone Deployment](deployment/standalone.md).

## Next Steps

After finishing this guide:

- Read [Standalone Deployment](deployment/standalone.md) for common operation commands.
- Read [Cluster Deployment](deployment/cluster.md) before preparing a multi-replica production setup.
