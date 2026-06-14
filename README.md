<div align="center">

# NeoGate

🚀 **极致性能、简单易用、企业私有化的大模型 API 网关**

Self-hosted Rust LLM API gateway for OpenAI-compatible and Anthropic-compatible APIs, model routing, multi-tenant API keys, usage tracking, billing, and enterprise private deployment.

<p align="center">
  <strong>中文</strong> |
  <a href="README.en.md">English</a>
</p>

[![License](https://img.shields.io/badge/license-AGPL--3.0-brightgreen.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/neogate-io/NeoGate?color=brightgreen&include_prereleases)](https://github.com/neogate-io/NeoGate/releases/latest)
[![Rust](https://img.shields.io/badge/rust-1.94%2B-orange.svg)](backend/Cargo.toml)
[![Docker](https://img.shields.io/badge/docker-compose-blue.svg)](docker-compose.yml)

<p align="center">
  <a href="#-功能概览">功能概览</a> •
  <a href="#-为什么选择-neogate">为什么选择 NeoGate</a> •
  <a href="#-快速开始">快速开始</a> •
  <a href="#-部署模式">部署模式</a> •
  <a href="docs/README.md">文档</a> •
  <a href="#-生产建议">生产建议</a> •
  <a href="#-获取帮助">获取帮助</a>
</p>

</div>

---

## 📝 项目介绍

NeoGate 是一个使用 Rust 构建的大模型 API 网关，面向企业私有化部署场景，目标是提供极致的请求转发性能，并保持简单易用。它帮助团队把多个模型供应商收拢到统一入口，集中管理访问密钥、模型路由、项目用量和成本归集。

仓库地址：[neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

> [!IMPORTANT]
> NeoGate 适用于合法、授权的 AI API 网关、企业级鉴权、多模型管理、用量统计、成本归集和私有化部署场景。使用者应合法获取上游 API key、账号、模型服务和接口权限，并遵守上游服务条款及所在地法律法规。

---

## 🔎 Search Keywords

`LLM API gateway` · `AI gateway` · `OpenAI-compatible proxy` · `Anthropic-compatible API` · `self-hosted AI infrastructure` · `model routing` · `multi-tenant API keys` · `usage tracking` · `cost management` · `billing` · `Rust`

---

## ✨ 功能概览

<table>
  <thead>
    <tr>
      <th width="180">能力</th>
      <th>说明</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🏢 企业统一入口</td>
      <td>将多个模型供应商统一收拢到企业自己的大模型 API 入口，由网关集中托管上游凭证和访问策略。</td>
    </tr>
    <tr>
      <td>🧩 项目化管理</td>
      <td>以项目作为业务应用、内部项目或成本单元，统一管理成员、API key、模型权限、预算额度和用量归集。</td>
    </tr>
    <tr>
      <td>🔑 独立 API Key</td>
      <td>给不同团队、项目、客户或内部应用分发独立 API key，支持按项目和 API key 隔离调用权限、额度和成本。</td>
    </tr>
    <tr>
      <td>🧭 模型路由</td>
      <td>在一个后台里集中管理 OpenAI、Anthropic 等上游模型服务，并按模型、优先级和权重分配请求。</td>
    </tr>
    <tr>
      <td>🔌 兼容接口</td>
      <td>对外提供 OpenAI 兼容和 Anthropic 兼容接口，让现有客户端少改配置即可接入企业统一网关。</td>
    </tr>
    <tr>
      <td>📊 用量记录</td>
      <td>记录用户、项目、API key、模型和上游通道维度的调用用量，方便排查问题、分析成本、内部核算和后续计费。</td>
    </tr>
    <tr>
      <td>💳 服务计费</td>
      <td>支持内部模式和计费模式，既可作为企业内部网关使用，也可开启额度、充值和支付能力面向客户或开发者收费。</td>
    </tr>
    <tr>
      <td>🛡️ 故障切换</td>
      <td>在上游 key 失败时自动冷却并切换可用 key，减少单个密钥或渠道异常对企业业务连续性的影响。</td>
    </tr>
  </tbody>
</table>

---

## 🧠 为什么选择 NeoGate

- **Rust 后端**：面向低延迟、高并发的大模型 API 转发场景设计。
- **私有化优先**：默认自托管部署，适合企业内部网关、团队共享上游账号和受控访问。
- **统一兼容接口**：同时提供 OpenAI 兼容和 Anthropic 兼容接口，方便现有客户端迁移。
- **项目和 Key 隔离**：按项目、成员和 API key 管理权限、额度、用量和成本归集。
- **内置计费模式**：支持内部模式和面向客户/开发者的计费模式，减少二次开发。
- **从单机到集群**：Docker Compose 快速启动，后续可切换到 Redis 协调的集群部署。

---

## 🧭 服务模式

NeoGate 首次运行时需要选择内部模式或计费模式。两种模式都支持统一入口、上游凭证集中托管、模型路由和用量记录，主要区别在于是否要求用户先有额度、是否接入支付通道。

<table>
  <thead>
    <tr>
      <th width="150">模式</th>
      <th>适用场景</th>
      <th width="180">调用限制</th>
      <th>配置重点</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🏠 内部模式</td>
      <td>公司、部门、项目组自用，或给内部应用、自动化脚本和成员分发 API key。</td>
      <td>默认不要求可用额度即可调用。</td>
      <td>仍会记录用量和费用，便于成本分析和内部管理。</td>
    </tr>
    <tr>
      <td>💰 计费模式</td>
      <td>面向客户、开发者或外部用户提供收费模型调用服务。</td>
      <td>用户需要有可用额度后才能调用。</td>
      <td>上线前需要配置模型价格、充值套餐和支付通道。</td>
    </tr>
  </tbody>
</table>

---

## 🚀 快速开始

### 🐳 Docker 安装

Docker 安装分为单机部署和集群部署。两种方式都不需要在宿主机单独安装 Rust、Node.js 或 pnpm。

#### 单机部署

单机部署适合大多数起步场景。Compose 会同时启动前端 Nginx、后端和 PostgreSQL，不需要额外准备 PostgreSQL 或 Redis。

```bash
docker compose up -d --build
```

启动后访问 `http://服务器IP:8080`，通过首次运行向导完成管理员、服务模式、初始上游、价格、SMTP 和支付等配置。

#### 集群部署

集群部署适合已经明确需要多副本和横向扩展的场景。集群版 Compose 只包含前端 Nginx、后端 API 和 worker，不包含 PostgreSQL/Redis，需要先准备外部 PostgreSQL、Redis、公开域名和共享密钥。

```bash
cp deploy/env/cluster.env.example .env.cluster
docker compose --env-file .env.cluster -f docker-compose.cluster.yml up -d --build
```

> [!WARNING]
> 生产环境请替换 `.env.cluster` 中的默认密码、域名和共享密钥。单机部署缺失的后端密钥可由首次运行向导自动生成并写入后端配置卷。

### 🧑‍💻 源码本地运行

源码本地运行分为开发部署和正式部署。开发部署适合调试或体验首次运行流程；正式部署适合希望从源码构建后自行托管进程和 Nginx 的场景。生产环境仍建议优先使用 Docker Compose。

#### 公共准备

先在服务器上准备这些依赖：

| 依赖 | 推荐版本 |
| --- | --- |
| PostgreSQL | 16 或兼容版本 |
| Rust | 1.94 或更新版本 |
| Node.js | 20 或兼容版本 |
| pnpm | 与项目前端兼容的版本 |

确认这些命令可用：

```bash
psql --version
cargo --version
node --version
pnpm --version
```

> [!TIP]
> 如果 `cargo --version` 显示的是 1.75 等较旧版本，请升级 Rust 工具链后再运行后端；旧版 Cargo 无法编译部分依赖。

建议创建 NeoGate 专用数据库用户和数据库：

```bash
sudo -u postgres psql
```

在 `psql` 中执行：

```sql
CREATE USER neogate WITH PASSWORD 'change-me';
CREATE DATABASE neogate OWNER neogate;
\q
```

首次运行向导里的 PostgreSQL 连接地址填写：

```text
postgres://neogate:change-me@localhost:5432/neogate
```

首次启动时，如果运行配置还不完整，后端会进入 bootstrap 模式，并通过首次运行页面写入数据库连接、站点信息和随机密钥。通常不需要先手动编辑 `backend/.env`。

#### 开发部署

开发部署使用 Rust 调试构建和 Vite 开发服务，适合本地开发、调试和体验首次运行流程。

<table>
  <thead>
    <tr>
      <th width="120">服务</th>
      <th>命令</th>
      <th width="190">默认地址</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>后端</td>
      <td><code>cd backend &amp;&amp; cargo run</code></td>
      <td><code>http://127.0.0.1:8080</code></td>
    </tr>
    <tr>
      <td>前端</td>
      <td><code>cd frontend &amp;&amp; pnpm install &amp;&amp; pnpm dev --host 0.0.0.0</code></td>
      <td><code>http://服务器IP:5173</code></td>
    </tr>
  </tbody>
</table>

打开 `http://服务器IP:5173`，页面会自动跳转到首次运行向导。按提示完成运行配置、管理员账号、服务模式、初始上游和可选 SMTP；如果保存运行配置后提示需要重启，请重新运行后端并刷新页面。

#### 正式部署

正式部署时建议使用 release 构建运行后端，并用 Nginx 托管前端静态文件。后端可交给 systemd、supervisord 或其他进程管理工具保持常驻。

构建后端：

```bash
cd backend
cargo build --release
```

运行后端：

```bash
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

也可以使用 systemd 托管后端进程。下面示例假设项目放在 `/opt/neogate`：

```ini
[Unit]
Description=NeoGate backend

[Service]
WorkingDirectory=/opt/neogate/backend
Environment=BIND_ADDR=127.0.0.1:8080
Environment=RUST_LOG=info
ExecStart=/opt/neogate/backend/target/release/neogate
Restart=always
StandardOutput=append:/var/log/neogate/backend.log
StandardError=append:/var/log/neogate/error.log

[Install]
WantedBy=multi-user.target
```

保存为 `/etc/systemd/system/neogate.service` 后启动：

```bash
sudo mkdir -p /var/log/neogate
sudo systemctl daemon-reload
sudo systemctl enable --now neogate
sudo systemctl status neogate
```

建议为后端日志添加 logrotate，避免日志文件长期运行后持续增长：

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

构建前端：

```bash
cd frontend
pnpm install
pnpm build
```

构建完成后，将 `frontend/dist` 交给 Nginx 等静态 Web 服务托管。仓库提供的 `deploy/nginx/standalone.conf.example` 默认以 `/usr/share/nginx/html` 为静态目录，并将后端接口和健康检查路径转发到本机后端 `http://127.0.0.1:8080`。

源码部署时可以按下面方式使用：

```bash
sudo install -d /usr/share/nginx/html
sudo cp -r frontend/dist/. /usr/share/nginx/html/
sudo cp deploy/nginx/standalone.conf.example /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

---

## 🏗️ 部署模式

NeoGate 可以按单节点或集群方式部署。大多数团队起步时使用单节点部署就够了：不需要 Redis，配置简单，部署和排障成本也更低。

<table>
  <thead>
    <tr>
      <th width="150">部署方式</th>
      <th width="190">配置</th>
      <th>适合场景</th>
      <th>组件</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🧱 单节点部署</td>
      <td>默认模式，无需配置 <code>RUNTIME_MODE</code>。</td>
      <td>个人项目、小团队和早期生产环境。</td>
      <td><code>docker-compose.yml</code> 会同时启动前端 Nginx、后端和 PostgreSQL。</td>
    </tr>
    <tr>
      <td>🌐 集群部署</td>
      <td>设置 <code>RUNTIME_MODE=distributed</code>。</td>
      <td>明确需要多副本和横向扩展的场景。</td>
      <td>多个后端 API/worker 共享 PostgreSQL 和 Redis；<code>docker-compose.cluster.yml</code> 不包含 PostgreSQL/Redis。</td>
    </tr>
  </tbody>
</table>

> [!TIP]
> 没有明确的多副本需求时，建议优先使用单节点部署。

---

## ✅ 生产建议

上线前必须确认：

<table>
  <thead>
    <tr>
      <th width="170">检查项</th>
      <th>建议</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>👤 管理员账号</td>
      <td>在首次运行向导中创建管理员账号，不要使用弱密码。</td>
    </tr>
    <tr>
      <td>🔐 系统密钥</td>
      <td>使用足够长且随机的 <code>ADMIN_TOKEN_SECRET</code> 和 <code>UPSTREAM_SECRET_KEY</code>；单机部署可由首次运行向导生成，集群部署需要提前写入所有节点共享的环境配置。</td>
    </tr>
    <tr>
      <td>🌍 站点地址</td>
      <td>在首次运行向导或环境配置中设置可信的 <code>PUBLIC_BASE_URL</code>，用于生成密码重置链接和安装脚本地址。</td>
    </tr>
    <tr>
      <td>🏷️ 站点名称</td>
      <td>在首次运行向导或环境配置中设置 <code>SITE_NAME</code>，用于页面、邮件和支付网关显示。</td>
    </tr>
  </tbody>
</table>

按使用场景确认：

<table>
  <thead>
    <tr>
      <th width="170">场景</th>
      <th>建议</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>🔁 跨域访问</td>
      <td>当前端与 API 跨域访问时，设置正确的 <code>CORS_ALLOWED_ORIGINS</code>；同域反向代理部署通常无需额外配置。</td>
    </tr>
    <tr>
      <td>📦 大请求转发</td>
      <td>转发图片编辑、文件上传或超长上下文请求时，确认反向代理的请求体限制不低于后端 <code>RELAY_BODY_LIMIT_BYTES</code>（默认 64 MiB）。</td>
    </tr>
    <tr>
      <td>🧾 计费用量解析</td>
      <td><code>RELAY_USAGE_BUFFER_LIMIT_BYTES</code> 默认 16 MiB，用于非流式 JSON 和 SSE 用量解析；计费模式建议保持默认或按最大响应体压测后再调整，避免影响计费用量提取。</td>
    </tr>
    <tr>
      <td>⏱️ 长耗时请求</td>
      <td>图片编辑等长耗时请求如果出现 504，可调大 <code>UPSTREAM_TIMEOUT_SECONDS</code>（默认 600 秒；旧的 <code>REQUEST_TIMEOUT_SECONDS</code> 仍作为兼容别名）。</td>
    </tr>
    <tr>
      <td>✉️ 邮箱领 key</td>
      <td>如需公开邮箱领取 API key，在首次运行向导或管理员后台的系统设置中配置 SMTP。</td>
    </tr>
    <tr>
      <td>💳 计费模式</td>
      <td>如需使用计费模式，在首次运行向导或管理员后台配置模型价格、充值套餐和支付通道。</td>
    </tr>
    <tr>
      <td>🌐 集群部署</td>
      <td>如需集群部署，设置 <code>RUNTIME_MODE=distributed</code> 并配置 Redis；否则保持默认单节点模式即可。</td>
    </tr>
  </tbody>
</table>

---

## 📄 开源协议

NeoGate 社区版使用 [GNU Affero General Public License v3.0](LICENSE) 开源协议（`AGPL-3.0-only`）。

NeoGate 同时提供分层商业授权：普通企业内部使用可申请免费的书面 Internal Commercial License；客户交付、托管服务、SaaS、OEM、MSP、白标、转售等场景需要单独 Commercial License；企业版功能使用商业 EULA。详见 [LICENSING.md](LICENSING.md)。

NeoGate 名称、Logo 及相关标识不随 AGPL 授权，使用边界见 [TRADEMARKS.md](TRADEMARKS.md)。

---

## 🙋 获取帮助

| 类型 | 链接 |
| --- | --- |
| 🐛 问题反馈 | [GitHub Issues](https://github.com/neogate-io/NeoGate/issues) |
| 🤝 代码贡献 | [Pull Requests](https://github.com/neogate-io/NeoGate/pulls) |
| 💬 官方 QQ 群 | 群号：`1179649618` |

<p align="left">
  <img src="frontend/public/qrcode.png" alt="NeoGate 官方 QQ 群二维码" width="220" />
</p>
