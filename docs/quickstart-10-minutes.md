# Deploy NeoGate in 10 Minutes

This is the shortest Docker Compose path for trying NeoGate for the first time. For complete deployment notes, see [Standalone Deployment](deployment/standalone.md) and [Cluster Deployment](deployment/cluster.md).

## Prerequisites

You need:

- A machine that can run Docker and Docker Compose.
- A valid upstream model API key.
- Firewall or security group access to port `8080`.

Confirm Docker is available:

```bash
docker --version
docker compose version
```

## 1. Start Services

Run this from the NeoGate repository root:

```bash
docker compose up -d
```

This starts PostgreSQL, the backend, and the frontend Nginx service.

Check container status:

```bash
docker compose ps
```

You should normally see `postgres`, `backend`, and `web` in `running` or `healthy` state.

View logs:

```bash
docker compose logs -f
```

View backend logs only:

```bash
docker compose logs -f backend
```

## 2. Optional: Bind A Domain With Host Nginx

Docker Compose exposes NeoGate on host port `8080` by default. If you access NeoGate through `http://SERVER_IP:8080`, you can skip this step.

If you want to bind a domain, configure host Nginx to reverse proxy to:

```text
http://127.0.0.1:8080
```

The repository includes an example config:

```bash
sudo cp deploy/nginx/docker-compose.conf.example /etc/nginx/conf.d/neogate.conf
sudo vim /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

When editing the config, check the domain, certificate paths, and proxy target.

## 3. Complete The Bootstrap Wizard

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

For internal evaluation, choose internal mode first. Internal mode does not require users or projects to have available credit before requests can be sent.

## 4. Check The Upstream Channel

After entering the admin console, open:

```text
Admin Console -> Upstream Channels
```

Make sure at least one channel is enabled, then check:

- The base URL is correct.
- The API key is valid.
- The model list contains the model you plan to call.
- Channel diagnostics pass.

If diagnostics fail, check the API key, model name, base URL, server network, and provider protocol compatibility first.

## 5. Create Or Copy An API Key

In internal mode, projects are the recommended way to manage teams or business apps:

```text
Admin Console -> Projects
```

Create a project, for example:

```text
Internal AI Gateway
```

Then create or view an API key for a project member. You can also open:

```text
User Console -> API Keys
```

Copy a usable NeoGate API key. Your application will call NeoGate with this key instead of using the upstream provider key directly.

## 6. Send A Test Request

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

## 7. Optional: Automatically Configure Local Tools

If you want local Codex or Claude Code to use NeoGate directly, run the automatic configuration script provided by your NeoGate service.

Linux / macOS / WSL:

```bash
curl -fsSL http://SERVER_IP:8080/install | bash
```

Windows PowerShell:

```powershell
irm http://SERVER_IP:8080/install.ps1 | iex
```

If you have already bound a domain, replace `http://SERVER_IP:8080` with your NeoGate public URL.

The script guides you through:

- Verifying the NeoGate API key.
- Selecting the client to configure, such as Codex CLI or Claude Code.
- Choosing a model from the available model list.
- Reviewing the configuration summary.
- Writing the Base URL, API key, and model name.
- Running one gateway relay test.

If the machine is already configured for NeoGate, running the same command again tries to reuse the previous API key, model, and client, then asks whether to switch model or reinstall.
