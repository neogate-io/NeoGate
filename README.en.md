<div align="center">

# NeoGate

🚀 **An LLM API gateway built for maximum performance, simple operation, and enterprise private deployment**

Self-hosted Rust LLM API gateway for OpenAI-compatible and Anthropic-compatible APIs, model routing, multi-tenant API keys, usage tracking, billing, and enterprise private deployment.

<p align="center">
  <a href="README.md">中文</a> |
  <strong>English</strong>
</p>

[![License](https://img.shields.io/badge/license-AGPL--3.0-brightgreen.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/neogate-io/NeoGate?color=brightgreen&include_prereleases)](https://github.com/neogate-io/NeoGate/releases/latest)
[![Rust](https://img.shields.io/badge/rust-1.94%2B-orange.svg)](backend/Cargo.toml)
[![Docker](https://img.shields.io/badge/docker-compose-blue.svg)](docker-compose.yml)

<p align="center">
  <a href="#-key-features">Key Features</a> •
  <a href="#-why-neogate">Why NeoGate</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="docs/README.md">Docs</a> •
  <a href="#-production-checklist">Production Checklist</a> •
  <a href="#-help">Help</a> •
  <a href="docs/deployment/cluster.md">Cluster Deployment</a>
</p>

</div>

---

## 📝 Project Introduction

NeoGate is an LLM API gateway built with Rust for enterprise private deployment. It is designed for maximum request forwarding performance while remaining simple to deploy and use. NeoGate helps teams bring multiple model providers behind one unified entry point for access keys, model routing, project usage, and cost attribution.

Repository: [neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

> [!IMPORTANT]
> NeoGate is intended for lawful and authorized AI API gateway, enterprise authentication, multi-model management, usage analytics, cost attribution, and private deployment scenarios. Users should lawfully obtain upstream API keys, accounts, model services, and API permissions, and comply with upstream terms of service and applicable laws and regulations.

---

## 🔎 Search Keywords

`LLM API gateway` · `AI gateway` · `OpenAI-compatible proxy` · `Anthropic-compatible API` · `self-hosted AI infrastructure` · `model routing` · `multi-tenant API keys` · `usage tracking` · `cost management` · `billing` · `Rust`

---

## ✨ Key Features

<table>
  <thead>
    <tr>
      <th width="210">Capability</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🏢 Enterprise gateway</td>
      <td>Deploy a privately controlled enterprise LLM API entry point that centralizes upstream credentials and access policies for multiple model providers.</td>
    </tr>
    <tr>
      <td>🧩 Project management</td>
      <td>Use projects as business applications, internal projects, or cost units, with unified management for members, API keys, model permissions, budgets, and usage attribution.</td>
    </tr>
    <tr>
      <td>🔑 Independent API keys</td>
      <td>Issue independent API keys to teams, projects, customers, or internal apps, with permissions, quota, and cost isolation by project and API key.</td>
    </tr>
    <tr>
      <td>🧭 Model routing</td>
      <td>Manage OpenAI, Anthropic, and other upstream model services from one admin console, and route requests by model, priority, and weight.</td>
    </tr>
    <tr>
      <td>🔌 Compatible APIs</td>
      <td>Expose OpenAI-compatible and Anthropic-compatible endpoints so existing clients can connect to the enterprise gateway with minimal changes.</td>
    </tr>
    <tr>
      <td>📊 Usage records</td>
      <td>Track usage by user, project, API key, model, and upstream channel for troubleshooting, cost analysis, internal chargeback, and future billing.</td>
    </tr>
    <tr>
      <td>💳 Service billing</td>
      <td>Choose internal mode or billing mode, using NeoGate either as an internal enterprise gateway or as a paid service with quota, recharge, and payment support.</td>
    </tr>
    <tr>
      <td>🛡️ Failover</td>
      <td>Cool down failing upstream keys and continue routing through available keys to reduce the impact of a single credential or channel failure.</td>
    </tr>
    <tr>
      <td>🚀 Cluster deployment</td>
      <td>Start with standalone Compose and move to a multi-replica production deployment backed by external PostgreSQL and Redis.</td>
    </tr>
  </tbody>
</table>

---

## 🧠 Why NeoGate

- **Rust backend**: Designed for low-latency, high-concurrency LLM API forwarding.
- **Self-hosted first**: Built for private enterprise deployment, shared upstream accounts, and controlled internal access.
- **Compatible APIs**: Provides OpenAI-compatible and Anthropic-compatible APIs for easier client migration.
- **Project and key isolation**: Manage permissions, balance, usage, and cost attribution by project, member, and API key.
- **Built-in billing mode**: Supports both internal gateway usage and paid access for customers or developers.
- **Standalone to clustered**: Start with Docker Compose, then move to Redis-coordinated clustered deployment when needed.

---

## 🧭 Service Modes

NeoGate asks you to choose internal mode or billing mode during first-run setup. Both modes provide a unified entry point, centralized upstream credential management, model routing, and usage records. The main difference is whether users need credit balance and whether payment gateways are enabled.

<table>
  <thead>
    <tr>
      <th width="170">Mode</th>
      <th>Scenario</th>
      <th width="190">Call restriction</th>
      <th>Configuration focus</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🏠 Internal mode</td>
      <td>Company, department, or project team usage, including API keys issued to internal apps, automation scripts, and team members.</td>
      <td>By default, users can call without available credit.</td>
      <td>NeoGate still records usage and cost for analysis and internal management.</td>
    </tr>
    <tr>
      <td>💰 Billing mode</td>
      <td>Paid access offered to customers, developers, or external users.</td>
      <td>Users need available credit before calls.</td>
      <td>Configure model prices, recharge plans, and the payment gateway before going live.</td>
    </tr>
  </tbody>
</table>

---

## 🚀 Quick Start

### 🐳 Docker Installation

Docker installation does not require Rust, Node.js, or pnpm on the host. The flow below is the standalone deployment path for most starting scenarios. For multi-replica and horizontally scalable production environments, see the [cluster deployment guide](docs/deployment/cluster.md).

#### Standalone Deployment

Standalone deployment is suitable for most starting scenarios. Compose starts frontend Nginx, the backend, and PostgreSQL together, so you do not need to prepare PostgreSQL or Redis separately.

```bash
# Overseas (Docker Hub directly accessible)
docker compose up -d --build

# Mainland China (uses domestic mirrors)
docker compose -f docker-compose.cn.yml up -d --build
```

After startup, open `http://SERVER_IP:8080` and complete the first-run wizard for the admin account, service mode, initial upstream, prices, SMTP, and payment settings.

#### Check Runtime Status

After standalone deployment starts, check whether all containers are `running` or `healthy`:

```bash
# Overseas
docker compose ps

# Mainland China
docker compose -f docker-compose.cn.yml ps
```

You should normally see `postgres`, `backend`, and `web`. If a service is not `running` or `healthy`, check logs:

```bash
# Overseas
docker compose logs -f

# Mainland China
docker compose -f docker-compose.cn.yml logs -f
```

You can also inspect one service, such as the standalone backend:

```bash
# Overseas
docker compose logs -f backend

# Mainland China
docker compose -f docker-compose.cn.yml logs -f backend
```

Finally, open `http://SERVER_IP:8080` in a browser. If the first-run wizard or login page loads, the frontend, backend, and reverse proxy path are usually working.

### 🧑‍💻 Local Source Run

Running from source is split into development deployment and production deployment. Development deployment is for debugging or trying the first-run flow; production deployment is for users who want to build from source and manage the backend process and Nginx themselves. For production environments, Docker Compose is still recommended first.

#### Shared Preparation

Prepare these dependencies on the server first:

| Dependency | Recommended version |
| --- | --- |
| PostgreSQL | 16 or a compatible version |
| Rust | 1.94 or newer |
| Node.js | 20 or a compatible version |
| pnpm | A version compatible with the frontend project |

Verify that these commands are available:

```bash
psql --version
cargo --version
node --version
pnpm --version
```

> [!TIP]
> If `cargo --version` shows an older version such as 1.75, upgrade the Rust toolchain before starting the backend; older Cargo versions cannot build some dependencies.

Create a dedicated NeoGate database user and database:

```bash
sudo -u postgres psql
```

Run this in `psql`:

```sql
CREATE USER neogate WITH PASSWORD 'change-me';
CREATE DATABASE neogate OWNER neogate;
\q
```

Use this PostgreSQL connection URL in the first-run wizard:

```text
postgres://neogate:change-me@localhost:5432/neogate
```

On first startup, if runtime configuration is incomplete, the backend enters bootstrap mode. The first-run page writes the database connection, site identity, and generated secrets for you. In the usual standalone flow, you do not need to edit `.env` before starting.

#### Development Deployment

Development deployment uses the Rust debug build and the Vite dev server. Use it for local development, debugging, and trying the first-run flow.

<table>
  <thead>
    <tr>
      <th width="120">Service</th>
      <th>Command</th>
      <th width="190">Default URL</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Backend</td>
      <td><code>cargo run -p neogate</code></td>
      <td><code>http://127.0.0.1:8080</code></td>
    </tr>
    <tr>
      <td>Scheduled jobs</td>
      <td><code>cargo run -p neogate-scheduler</code></td>
      <td>No HTTP URL</td>
    </tr>
    <tr>
      <td>Frontend</td>
      <td><code>cd frontend &amp;&amp; pnpm install &amp;&amp; pnpm dev --host 0.0.0.0</code></td>
      <td><code>http://SERVER_IP:5173</code></td>
    </tr>
  </tbody>
</table>

Open `http://SERVER_IP:5173`; the app redirects to the first-run wizard automatically. Complete the runtime configuration, admin account, service mode, initial upstream, and optional SMTP settings. If the page asks for a restart after saving runtime configuration, restart the backend and refresh the page.

#### Production Deployment

For production deployment, build the backend and scheduled jobs in release mode and serve the frontend static files with Nginx. Use systemd, supervisord, or another process manager to keep the backend and scheduled jobs running.

Build the backend and scheduled jobs:

```bash
cargo build --release -p neogate -p neogate-scheduler
```

Run the backend:

```bash
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

Run the scheduled jobs:

```bash
./target/release/neogate-scheduler
```

You can also run the backend with systemd and have it start the scheduled jobs automatically. This example assumes the project is located at `/opt/neogate` and the release build was created from the repository root:

```ini
[Unit]
Description=NeoGate backend

[Service]
WorkingDirectory=/opt/neogate
Environment=BIND_ADDR=127.0.0.1:8080
Environment=RUST_LOG=info
Environment=NEOGATE_ENV_FILE=/opt/neogate/.env
ExecStart=/opt/neogate/deploy/systemd/start-neogate.sh
KillMode=control-group
TimeoutStopSec=30
Restart=always
StandardOutput=append:/var/log/neogate/backend.log
StandardError=append:/var/log/neogate/backend-error.log

[Install]
WantedBy=multi-user.target
```

Save it as `/etc/systemd/system/neogate.service`, then start it:

```bash
sudo mkdir -p /var/log/neogate
sudo systemctl daemon-reload
sudo systemctl enable --now neogate
sudo systemctl status neogate
```

Add logrotate for service logs to avoid unbounded log file growth:

```bash
sudo tee /etc/logrotate.d/neogate >/dev/null <<'EOF'
/var/log/neogate/*.log {
    daily
    rotate 14
    compress
    missingok
    notifempty
    copytruncate
}
EOF
```

Build the frontend:

```bash
cd frontend
pnpm install
pnpm build
```

After the build, serve `frontend/dist` with Nginx or another static web server. The included `deploy/nginx/standalone.conf.example` uses `/usr/share/nginx/html` as the static file root and proxies backend APIs and health-check paths to the local backend at `http://127.0.0.1:8080`.

For source deployments, you can use it like this:

```bash
sudo install -d /usr/share/nginx/html
sudo cp -r frontend/dist/. /usr/share/nginx/html/
sudo cp deploy/nginx/standalone.conf.example /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

## ✅ Production Checklist

Required before going live:

<table>
  <thead>
    <tr>
      <th width="190">Check</th>
      <th>Recommendation</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>👤 Admin account</td>
      <td>Create the admin account in the first-run wizard and avoid weak passwords.</td>
    </tr>
    <tr>
      <td>🔐 System secrets</td>
      <td>Use long, random values for <code>ADMIN_TOKEN_SECRET</code> and <code>UPSTREAM_SECRET_KEY</code>; the first-run wizard can generate them for standalone deployments, while clustered deployments need shared values in the environment for every node.</td>
    </tr>
    <tr>
      <td>🌍 Public URL</td>
      <td>Set a trusted <code>PUBLIC_BASE_URL</code> in the first-run wizard or environment configuration for password reset links and install script URLs.</td>
    </tr>
    <tr>
      <td>🏷️ Site name</td>
      <td>Set <code>SITE_NAME</code> in the first-run wizard or environment configuration for page, email, and payment gateway display.</td>
    </tr>
  </tbody>
</table>

Check by scenario:

<table>
  <thead>
    <tr>
      <th width="210">Scenario</th>
      <th>Recommendation</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🔁 Cross-origin access</td>
      <td>When the frontend and API are accessed cross-origin, set the correct <code>CORS_ALLOWED_ORIGINS</code>; same-origin reverse proxy deployments usually do not need extra CORS configuration.</td>
    </tr>
    <tr>
      <td>📦 Large request proxying</td>
      <td>When proxying image edits, file uploads, or very long context requests, make sure the reverse proxy request-body limit is at least the backend <code>RELAY_BODY_LIMIT_BYTES</code> value, which defaults to 64 MiB.</td>
    </tr>
    <tr>
      <td>🧾 Billing usage parsing</td>
      <td><code>RELAY_USAGE_BUFFER_LIMIT_BYTES</code> defaults to 16 MiB for non-streaming JSON and SSE usage parsing; keep the default in billing mode unless load tests prove a different limit still preserves usage extraction.</td>
    </tr>
    <tr>
      <td>⏱️ Long-running requests</td>
      <td>For 504s on long image edits, increase <code>UPSTREAM_TIMEOUT_SECONDS</code>; it defaults to 600 seconds, and the old <code>REQUEST_TIMEOUT_SECONDS</code> name remains a compatibility alias.</td>
    </tr>
    <tr>
      <td>🩺 Upstream monitoring</td>
      <td>Channel availability is probed by the scheduled jobs process every 10 minutes by default; adjust <code>CHANNEL_PROBE_INTERVAL_SECONDS</code> if needed. Upstream model lists are synced once per day by default; adjust <code>UPSTREAM_MODEL_SYNC_INTERVAL_SECONDS</code> if needed.</td>
    </tr>
    <tr>
      <td>💳 Billing mode</td>
      <td>For billing mode, configure model prices, recharge plans, and the payment gateway in the first-run wizard or admin console.</td>
    </tr>
    <tr>
      <td>🌐 Clustered deployment</td>
      <td>For clustered deployment, set <code>RUNTIME_MODE=distributed</code> and configure Redis. Otherwise, keep the default single-node mode.</td>
    </tr>
  </tbody>
</table>

---

## 📄 License

NeoGate Community Edition is licensed under the [GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`).

NeoGate also offers tiered commercial licensing: ordinary companies may request a free written Internal Commercial License for internal use; customer delivery, managed service, SaaS, OEM, MSP, white-label, resale, and similar third-party use require a separate Commercial License; Enterprise Edition features are governed by a commercial EULA. See [LICENSING.md](LICENSING.md).

The NeoGate name, logo, and related marks are not licensed under AGPL. Their use is governed by [TRADEMARKS.md](TRADEMARKS.md).

---

## 🙋 Help

| Type | Link |
| --- | --- |
| 🐛 Issues | [GitHub Issues](https://github.com/neogate-io/NeoGate/issues) |
| 🤝 Contributions | [Pull Requests](https://github.com/neogate-io/NeoGate/pulls) |
| 💬 Official QQ group | `1179649618` |

<p align="left">
  <img src="frontend/public/qrcode.png" alt="NeoGate official QQ group QR code" width="220" />
</p>
