<div align="center">

# NeoGate

🚀 **极致性能、简单易用、企业私有化的大模型 API 网关**

Self-hosted Rust LLM API gateway for OpenAI-compatible and Anthropic-compatible APIs, model routing, app publishing to WeCom, webhooks, web widgets, multi-tenant API keys, usage tracking, billing, and enterprise private deployment.

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
  <a href="docs/README.md">文档</a> •
  <a href="#-生产建议">生产建议</a> •
  <a href="#-获取帮助">获取帮助</a> •
  <a href="docs/deployment/cluster.zh.md">集群部署</a>
</p>

</div>

---

## 📝 项目介绍

NeoGate 是一个使用 Rust 构建的大模型 API 网关，面向企业私有化部署场景，强调极致性能、简单易用和可控运维。它帮助企业把大模型调用统一到可管理、可观测、可计费的网关之下。

通过「应用管理」，NeoGate 可以把大模型能力发布到企业微信、Webhook 和网页组件等入口，让员工、业务系统或外部应用直接使用 AI 能力。NeoGate 适用于企业内部 AI 网关、多项目多团队统一管理、多应用接入，以及面向客户或开发者的计费运营场景。

仓库地址：[neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

> [!IMPORTANT]
> NeoGate 适用于合法、授权的 AI API 网关、企业级鉴权、多模型管理、用量统计、成本归集和私有化部署场景。使用者应合法获取上游 API key、账号、模型服务和接口权限，并遵守上游服务条款及所在地法律法规。

---

## 🔎 Search Keywords

`LLM API gateway` · `AI gateway` · `OpenAI-compatible proxy` · `Anthropic-compatible API` · `self-hosted AI infrastructure` · `model routing` · `AI app management` · `WeCom integration` · `webhook AI app` · `web chat widget` · `multi-tenant API keys` · `usage tracking` · `cost management` · `billing` · `Rust`

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
      <td>提供企业自己的大模型 API 入口，兼容 OpenAI 和 Anthropic 接口，并集中托管上游凭证和访问策略。</td>
    </tr>
    <tr>
      <td>🧩 项目化管理</td>
      <td>以项目承载业务、团队或成本单元，管理成员、模型权限、预算额度和成本归集。</td>
    </tr>
    <tr>
      <td>🔑 独立 API Key</td>
      <td>为团队、客户或内部应用分发独立 API Key，隔离调用权限、额度和用量追踪。</td>
    </tr>
    <tr>
      <td>🧰 应用管理</td>
      <td>创建企业微信、Webhook 和网页组件应用，让员工或外部系统直接与大模型对话。</td>
    </tr>
    <tr>
      <td>🧭 模型路由</td>
      <td>按模型、优先级和权重分配请求，并在上游 key 异常时自动冷却和切换。</td>
    </tr>
    <tr>
      <td>📊 用量记录</td>
      <td>记录用户、项目、API Key、模型和通道维度的调用用量，便于排障、分析和核算。</td>
    </tr>
    <tr>
      <td>💳 服务计费</td>
      <td>支持内部模式和计费模式，可开启额度、充值和支付能力，对内管理或对外收费。</td>
    </tr>
    <tr>
      <td>🚀 集群部署</td>
      <td>支持单机 Compose 部署，也支持基于外部 PostgreSQL 和 Redis 的生产多副本部署。</td>
    </tr>
  </tbody>
</table>

---

## 🧠 为什么选择 NeoGate

- **接入更省心**：现有客户端和内部服务少改配置，就能统一接入大模型能力，把试用和上线周期都缩短。
- **密钥更安全**：上游凭证不用散落在各个应用里，权限、额度和访问策略集中管理，团队用起来更安心。
- **成本更清楚**：谁在用、哪个项目在用、用了多少 Token 和费用，都能按维度看清楚，核算和排查更顺手。
- **团队更好协作**：不同团队、客户或内部应用可以用项目隔开，边界清楚，统计清楚，协作也更轻松。
- **AI 更容易落地**：通过企业微信、Webhook 和网页组件，把大模型能力放到员工和系统每天都会用到的入口里。
- **场景覆盖更完整**：既能做企业内部 AI 网关，也能延伸到额度、充值、支付和计费运营，少做很多重复建设。

---

## 🧭 服务模式

NeoGate 首次运行时需要选择内部模式或计费模式。内部模式适合企业自用和团队协作，计费模式适合面向客户或开发者提供服务。两种模式都保留统一入口、凭证托管、模型路由和用量记录，主要区别在于是否要求可用额度，以及是否接入支付通道。

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
      <td>公司、部门或项目组自用，给内部应用、自动化脚本和成员分发 API Key。</td>
      <td>默认不要求可用额度即可调用。</td>
      <td>仍会记录用量和费用，便于成本分析和内部核算。</td>
    </tr>
    <tr>
      <td>💰 计费模式</td>
      <td>面向客户、开发者或外部用户提供收费的大模型调用服务。</td>
      <td>用户需要有可用额度后才能调用。</td>
      <td>上线前需要配置模型价格、充值套餐和支付通道。</td>
    </tr>
  </tbody>
</table>

---

## 🚀 快速开始

### 🐳 Docker 安装

Docker 安装是最快的部署方式，不需要在宿主机单独安装 Rust、Node.js 或 pnpm。需要多副本和横向扩展的生产环境，请查看：[集群部署文档](docs/deployment/cluster.zh.md)。

#### 单机部署

单机部署适合试用、内部部署和中小规模场景。Compose 会同时启动前端 Nginx、后端和 PostgreSQL，不需要额外准备 PostgreSQL 或 Redis。

```bash
# 海外（Docker Hub 可直接访问）
docker compose up -d --build

# 中国大陆（使用国内镜像源）
docker compose -f docker-compose.cn.yml up -d --build
```

启动后访问 `http://服务器IP:8080`，首次运行向导会引导你完成管理员、服务模式、初始上游、价格、SMTP 和支付等配置。

#### 绑定域名和宿主机 Nginx

使用 `docker compose up -d --build` 安装时，Compose 默认会把服务暴露到宿主机 `8080` 端口。宿主机 Nginx 可直接反向代理到 `http://127.0.0.1:8080`：

```bash
sudo cp deploy/nginx/docker-compose.conf.example /etc/nginx/conf.d/neogate.conf
sudo vim /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

#### 检查运行状态

部署完成后，可以先查看容器是否都处于 `running` 或 `healthy` 状态：

```bash
# 海外
docker compose ps

# 中国大陆
docker compose -f docker-compose.cn.yml ps
```

通常会看到 `postgres`、`backend`、`web` 三个服务。若服务状态不是 `running`/`healthy`，可以查看日志定位原因：

```bash
# 海外
docker compose logs -f

# 中国大陆
docker compose -f docker-compose.cn.yml logs -f
```

也可以只查看某个服务的日志，例如单机部署的后端：

```bash
# 海外
docker compose logs -f backend

# 中国大陆
docker compose -f docker-compose.cn.yml logs -f backend
```

最后在浏览器访问 `http://服务器IP:8080`。如果可以打开首次运行向导或登录页面，通常表示前端、后端和反向代理链路已经正常。

### 🧑‍💻 源码本地运行

源码运行适合二次开发、调试和自定义部署。你可以使用开发部署体验完整流程，也可以使用 release 构建配合 Nginx、systemd 等工具自行托管。生产环境仍建议优先使用 Docker Compose 或集群部署方案。

#### 公共准备

先准备这些依赖：

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

首次启动时，如果运行配置还不完整，后端会进入 bootstrap 模式，并通过首次运行页面写入数据库连接、站点信息和随机密钥。多数情况下不需要先手动编辑 `.env`。

#### 开发部署

开发部署使用 Rust 调试构建和 Vite 开发服务，适合本地开发、联调和体验首次运行流程。

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
      <td><code>cargo run -p neogate</code></td>
      <td><code>http://127.0.0.1:8080</code></td>
    </tr>
    <tr>
      <td>定时任务</td>
      <td><code>cargo run -p neogate-scheduler</code></td>
      <td>无 HTTP 地址</td>
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

正式部署建议使用 release 构建运行后端和定时任务，并用 Nginx 托管前端静态文件。后端和定时任务可以交给 systemd、supervisord 或其他进程管理工具保持常驻。

构建后端和定时任务：

```bash
cargo build --release -p neogate -p neogate-scheduler
```

运行后端：

```bash
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

运行定时任务：

```bash
./target/release/neogate-scheduler
```

也可以使用 systemd 托管后端进程，并在启动后端时自动拉起定时任务。下面示例假设项目放在 `/opt/neogate`，并已在仓库根目录完成 release 构建：

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

保存为 `/etc/systemd/system/neogate.service` 后启动：

```bash
sudo mkdir -p /var/log/neogate
sudo systemctl daemon-reload
sudo systemctl enable --now neogate
sudo systemctl status neogate
```

建议为服务日志添加 logrotate，避免日志文件长期运行后持续增长：

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

构建完成后，将 `frontend/dist` 交给 Nginx 等静态 Web 服务托管。仓库提供的 `deploy/nginx/source-build.conf.example` 默认以 `/usr/share/nginx/html` 为静态目录，并将后端接口和健康检查路径转发到本机后端 `http://127.0.0.1:8080`。

源码部署时可以按下面方式使用：

```bash
sudo install -d /usr/share/nginx/html
sudo cp -r frontend/dist/. /usr/share/nginx/html/
sudo cp deploy/nginx/source-build.conf.example /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

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
      <td>🔀 上游故障切换</td>
      <td>上游返回 429、5xx、529 或发生超时/连接类错误时，NeoGate 默认最多切换其他可用上游 5 次；可通过 <code>MAX_UPSTREAM_FAILOVERS</code> 调整，设为 0 可关闭。</td>
    </tr>
    <tr>
      <td>🩺 上游监控</td>
      <td>通道可用性由定时任务进程探测，默认每 10 分钟执行一次；可通过 <code>CHANNEL_PROBE_INTERVAL_SECONDS</code> 调整。上游模型列表默认每天同步一次，可通过 <code>UPSTREAM_MODEL_SYNC_INTERVAL_SECONDS</code> 调整。</td>
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
