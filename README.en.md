<div align="center">

# NeoGate

🚀 **A Rust-built LLM API gateway for maximum performance, ease of use, and enterprise private deployment**

Self-hosted Rust LLM API gateway for OpenAI-compatible and Anthropic-compatible APIs, smart model routing, audio and video processing, enterprise app publishing, multi-tenant API keys, usage tracking, billing, and private deployment.

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
  <a href="docs/quickstart-10-minutes.md">10-Minute Quickstart</a> •
  <a href="docs/README.md">Docs</a> •
  <a href="#-production-checklist">Production Checklist</a> •
  <a href="#-help">Help</a> •
  <a href="docs/deployment/cluster.md">Cluster Deployment</a>
</p>

</div>

---

## 📝 Project Introduction

NeoGate is a Rust-built LLM API gateway for enterprise private deployment, focused on maximum performance, ease of use, and operational control. It helps enterprises bring LLM calls into a gateway that is manageable, observable, and billable.

NeoGate runs on enterprise-owned servers, private clouds, or internal networks, centralizing upstream credentials, model access policies, project members and API keys, usage records, and cost data. Enterprises can keep existing OpenAI and Anthropic clients while establishing independent permission, budget, and cost boundaries for departments, projects, and internal applications, then scale from a standalone deployment to a multi-replica architecture as demand grows.

Repository: [neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

<p align="center">
  <img src="docs/assets/admin-upstream-channels.png" alt="NeoGate upstream channel management screenshot" width="920">
</p>

> [!IMPORTANT]
> NeoGate is intended for lawful and authorized AI API gateway, enterprise authentication, multi-model management, usage analytics, cost attribution, and private deployment scenarios. Users should lawfully obtain upstream API keys, accounts, model services, and API permissions, and comply with upstream terms of service and applicable laws and regulations.

---

## 🔎 Search Keywords

`LLM API gateway` · `AI gateway` · `OpenAI-compatible proxy` · `Anthropic-compatible API` · `realtime ASR` · `OpenAI video API` · `Alibaba Cloud Bailian` · `self-hosted AI infrastructure` · `smart model routing` · `AI app management` · `WeCom integration` · `Feishu integration` · `DingTalk integration` · `webhook AI app` · `web chat widget` · `multi-tenant API keys` · `usage tracking` · `cost management` · `billing` · `Rust`

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
      <td>Provide a private enterprise LLM API entry point with OpenAI and Anthropic compatibility, while centralizing upstream credentials and access policies.</td>
    </tr>
    <tr>
      <td>🧩 Project management</td>
      <td>Use projects for business units, teams, or cost centers, with members, model permissions, budgets, and cost attribution in one place.</td>
    </tr>
    <tr>
      <td>🔑 Independent API keys</td>
      <td>Issue independent API keys to teams, customers, or internal apps, with isolated permissions, quotas, and usage tracking.</td>
    </tr>
    <tr>
      <td>🧰 App management</td>
      <td>Create WeCom, Feishu, DingTalk, webhook, and web widget apps so employees or external systems can talk to models directly.</td>
    </tr>
    <tr>
      <td>🧠 Smart model routing</td>
      <td>Detect multimodal input, tool use, code, reasoning effort, context length, and other request signals, then select a model by complexity while retaining an explainable routing decision.</td>
    </tr>
    <tr>
      <td>🧭 Reliable upstream routing</td>
      <td>Route requests by model, priority, and weight, with automatic cooldown and switching when upstream keys fail.</td>
    </tr>
    <tr>
      <td>🎙️ Audio/video processing</td>
      <td>Provide OpenAI-compatible file transcription, Realtime speech recognition, and video generation APIs, with unified asynchronous task handling, result retrieval, failover, and billing by audio duration or video configuration.</td>
    </tr>
    <tr>
      <td>📊 Usage and cost analysis</td>
      <td>Track calls by user, project, API key, model, and channel, then aggregate cost, request count, success rate, and token usage with date filters, multi-level drilldowns, and CSV export.</td>
    </tr>
    <tr>
      <td>💳 Service billing</td>
      <td>Support internal and billing modes, plus project-, API-key-, and model-level credit, reservation settlement, recharge, payment, and an auditable ledger.</td>
    </tr>
    <tr>
      <td>🚀 Cluster deployment</td>
      <td>Support standalone Compose deployment and production multi-replica deployment backed by PostgreSQL and Redis.</td>
    </tr>
  </tbody>
</table>

---

## 🖼️ Screenshots

<p align="center">
  <img src="docs/assets/admin-usage-statistics.png" alt="NeoGate usage statistics screenshot" width="920">
</p>

<p align="center">
  <img src="docs/assets/admin-usage-records.png" alt="NeoGate usage records screenshot" width="920">
</p>

<p align="center">
  <img src="docs/assets/admin-channel-diagnostics.png" alt="NeoGate channel diagnostics screenshot" width="920">
</p>

---

## 🧠 Why NeoGate

- **Easier to adopt**: Existing clients and internal services can connect with only a few configuration changes.
- **Safer credentials**: Upstream secrets stay out of individual apps, while permissions, quotas, and policies stay in one place.
- **Clearer cost tracking**: See who used what, which project used it, and how many tokens and dollars were spent.
- **Better team boundaries**: Separate teams, customers, and internal apps by project, with clean isolation and reporting.
- **Easier AI rollout**: Publish model access to WeCom, Feishu, DingTalk, webhook, and web widget entry points your users already touch every day.
- **Broader operating model**: Run NeoGate as an internal AI gateway or extend it into quota, recharge, payment, and billing operations.

---

## 🧭 Service Modes

NeoGate asks you to choose internal mode or billing mode during first-run setup. Internal mode is for company self-use and team collaboration, while billing mode is for customer-facing or developer-facing services. Both modes keep a unified entry point, centralized credentials, model routing, and usage records. The difference is whether usable credit is required and whether payment gateways are enabled.

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
      <td>Company, department, or project team usage, including API keys for internal apps, automation scripts, and team members.</td>
      <td>By default, users can call without available credit.</td>
      <td>NeoGate still records usage and cost for analysis and internal accounting.</td>
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

If this is your first evaluation, start with the practical guide: [10-Minute NeoGate Quickstart](docs/quickstart-10-minutes.md).

### 🐳 Docker Installation

Docker installation is the fastest path and does not require Rust, Node.js, or pnpm on the host. For multi-replica and horizontally scalable production environments, see the [cluster deployment guide](docs/deployment/cluster.md).

#### Standalone Deployment

Standalone deployment is suitable for trials, internal deployments, and smaller environments. Compose starts frontend Nginx, the backend, and PostgreSQL together, so you do not need to prepare PostgreSQL or Redis separately.

```bash
# Use prebuilt images without compiling on the server
docker compose up -d
```

If you want to build images locally from source, or need the Mainland China mirror configuration, use:

```bash
# Overseas (Docker Hub directly accessible)
docker compose -f docker-compose.build.yml up -d --build

# Mainland China (uses domestic mirrors)
docker compose -f docker-compose.cn.yml up -d --build
```

After startup, open `http://SERVER_IP:8080`. The first-run wizard will guide you through the admin account, service mode, initial upstream, prices, SMTP, and payment settings.

#### Domain and Host Nginx

When installed with Docker Compose, Compose exposes the service on host port `8080` by default. Host Nginx can reverse proxy directly to `http://127.0.0.1:8080`:

```bash
sudo cp deploy/nginx/docker-compose.conf.example /etc/nginx/conf.d/neogate.conf
sudo vim /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

#### Check Runtime Status

After deployment starts, check whether all containers are `running` or `healthy`:

```bash
docker compose ps
```

You should normally see `postgres`, `backend`, and `web`. If a service is not `running` or `healthy`, check logs:

```bash
docker compose logs -f
```

You can also inspect one service, such as the standalone backend:

```bash
docker compose logs -f backend
```

Finally, open `http://SERVER_IP:8080` in a browser. If the first-run wizard or login page loads, the frontend, backend, and reverse proxy path are usually working.

#### Admin Password Recovery

If an administrator forgets the password or the account is locked, reset it inside the backend container:

```bash
docker compose exec backend neogate admin reset-password --username admin
```

If the admin username is not `admin`, replace `--username`.

### 🧑‍💻 Local Source Run

Running from source is suitable for development, debugging, and custom deployments. Install PostgreSQL 16, Rust 1.94+, Node.js 20+, and pnpm first.

#### Prepare the Database

Open PostgreSQL:

```bash
sudo -u postgres psql
```

```sql
CREATE USER neogate WITH PASSWORD 'change-me';
CREATE DATABASE neogate OWNER neogate;
\q
```

Database connection URL:

```text
postgres://neogate:change-me@localhost:5432/neogate
```

#### Development Run

Start the backend, scheduled jobs, and frontend separately:

```bash
cargo run -p neogate
```

```bash
cargo run -p neogate-scheduler
```

```bash
cd frontend
pnpm install
pnpm dev --host 0.0.0.0
```

Open `http://localhost:5173`; for remote development, use `http://SERVER_IP:5173`. Complete the database connection, administrator account, service mode, and initial upstream in the first-run wizard. If the page asks for a restart after saving the runtime configuration, restart the backend.

#### Production Run

Build the backend, scheduled jobs, and frontend:

```bash
cargo build --release -p neogate -p neogate-scheduler
cd frontend
pnpm install
pnpm build
cd ..
```

Start the backend and scheduled jobs separately:

```bash
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

```bash
./target/release/neogate-scheduler
```

In production, use systemd, supervisord, or another process manager to keep services running. Serve `frontend/dist` with Nginx, proxy backend requests, and enable HTTPS. Docker Compose or the cluster deployment path remains the recommended option for production environments.

---

## ✅ Production Checklist

Before going live, verify at least the following:

- **Security and domain**: Use a strong administrator password and randomly generated `ADMIN_TOKEN_SECRET` and `UPSTREAM_SECRET_KEY` values. Serve NeoGate over HTTPS and configure `PUBLIC_BASE_URL` and the reverse proxy correctly.
- **Upstream availability**: Use channel diagnostics to validate models, endpoints, and credentials. Keep the scheduled jobs process running for channel probes and model catalog synchronization.
- **Billing configuration**: Before enabling billing mode, verify model prices, credit policies, recharge plans, and payment callbacks. In internal mode, also confirm that usage and cost records are being collected.
- **Data persistence**: Persist and regularly back up PostgreSQL, runtime configuration, and system secrets. When using background image generation or other workflows that store response assets locally, persist `NEOGATE_ASSET_DIR` as well.
- **Cluster deployment**: Multi-replica deployments must share PostgreSQL, Redis, and system secrets. Plan API, Worker, and Scheduler roles according to the expected load.

For detailed configuration and capacity limits, see:

- [Standalone deployment](docs/deployment/standalone.md)
- [Cluster deployment](docs/deployment/cluster.md)

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

<table>
  <tr>
    <th>WeChat Contact</th>
    <th>Official QQ Group</th>
  </tr>
  <tr>
    <td><img src="docs/assets/wechat.png" alt="NeoGate WeChat contact QR code" width="220" /></td>
    <td><img src="docs/assets/qq.png" alt="NeoGate official QQ group QR code" width="220" /></td>
  </tr>
</table>
