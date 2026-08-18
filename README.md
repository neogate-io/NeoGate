<div align="center">

# NeoGate

🚀 **极致性能、简单易用、企业私有化的大模型 API 网关**

Self-hosted Rust LLM API gateway for OpenAI-compatible and Anthropic-compatible APIs, smart model routing, audio and video processing, enterprise app publishing, multi-tenant API keys, usage tracking, billing, and private deployment.

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
  <a href="docs/quickstart-10-minutes.zh.md">10 分钟部署</a> •
  <a href="docs/README.md">文档</a> •
  <a href="#-生产建议">生产建议</a> •
  <a href="#-获取帮助">获取帮助</a> •
  <a href="docs/deployment/cluster.zh.md">集群部署</a>
</p>

</div>

---

## 📝 项目介绍

NeoGate 是一个使用 Rust 构建的大模型 API 网关，面向企业私有化部署场景，强调极致性能、简单易用和可控运维。它帮助企业把大模型调用统一到可管理、可观测、可计费的网关之下。

NeoGate 可部署在企业自有服务器、私有云或内部网络中，将上游凭证、模型访问策略、项目成员与 API Key、调用记录和成本数据集中纳管。企业可以在兼容现有 OpenAI 和 Anthropic 客户端的前提下，为不同部门、项目和内部应用建立独立的权限、预算与成本边界，并根据业务规模从单机部署平滑扩展到多副本架构。

仓库地址：[neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

<p align="center">
  <img src="docs/assets/admin-upstream-channels.png" alt="NeoGate 上游服务管理截图" width="920">
</p>

> [!IMPORTANT]
> NeoGate 适用于合法、授权的 AI API 网关、企业级鉴权、多模型管理、用量统计、成本归集和私有化部署场景。使用者应合法获取上游 API key、账号、模型服务和接口权限，并遵守上游服务条款及所在地法律法规。

---

## 🔎 Search Keywords

`LLM API gateway` · `AI gateway` · `OpenAI-compatible proxy` · `Anthropic-compatible API` · `realtime ASR` · `OpenAI video API` · `Alibaba Cloud Bailian` · `self-hosted AI infrastructure` · `smart model routing` · `AI app management` · `WeCom integration` · `Feishu integration` · `DingTalk integration` · `webhook AI app` · `web chat widget` · `multi-tenant API keys` · `usage tracking` · `cost management` · `billing` · `Rust`

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
      <td>创建企业微信、飞书、钉钉、Webhook 和网页组件应用，让员工或外部系统直接与大模型对话。</td>
    </tr>
    <tr>
      <td>🧠 智能模型路由</td>
      <td>识别多模态、工具调用、代码、推理强度和上下文长度等请求特征，按复杂度选择不同模型，并保留可追踪的路由决策。</td>
    </tr>
    <tr>
      <td>🧭 可靠上游路由</td>
      <td>按模型、优先级和权重分配请求，并在上游 key 异常时自动冷却和切换。</td>
    </tr>
    <tr>
      <td>🎙️ 语音/视频处理</td>
      <td>提供 OpenAI 兼容的文件转写、Realtime 实时语音识别和视频生成接口，统一处理异步任务、结果查询、故障切换，以及按音频时长或视频规格计费。</td>
    </tr>
    <tr>
      <td>📊 用量与成本分析</td>
      <td>记录用户、项目、API Key、模型和通道维度的调用明细，并汇总成本、请求数、成功率与 Token 用量，支持时间筛选、多级下钻和 CSV 导出。</td>
    </tr>
    <tr>
      <td>💳 服务计费</td>
      <td>支持内部模式和计费模式，以及项目、API Key、模型三级额度、预留结算、充值支付和可追踪账本。</td>
    </tr>
    <tr>
      <td>🚀 集群部署</td>
      <td>支持单机 Compose 部署，也支持基于外部 PostgreSQL 和 Redis 的生产多副本部署。</td>
    </tr>
  </tbody>
</table>

---

## 🖼️ 界面预览

<p align="center">
  <img src="docs/assets/admin-usage-statistics.png" alt="NeoGate 消费统计截图" width="920">
</p>

<p align="center">
  <img src="docs/assets/admin-usage-records.png" alt="NeoGate 使用明细截图" width="920">
</p>

<p align="center">
  <img src="docs/assets/admin-channel-diagnostics.png" alt="NeoGate 通道诊断截图" width="920">
</p>

---

## 🧠 为什么选择 NeoGate

- **接入更省心**：现有客户端和内部服务少改配置，就能统一接入大模型能力，把试用和上线周期都缩短。
- **密钥更安全**：上游凭证不用散落在各个应用里，权限、额度和访问策略集中管理，团队用起来更安心。
- **成本更清楚**：谁在用、哪个项目在用、用了多少 Token 和费用，都能按维度看清楚，核算和排查更顺手。
- **团队更好协作**：不同团队、客户或内部应用可以用项目隔开，边界清楚，统计清楚，协作也更轻松。
- **AI 更容易落地**：通过企业微信、飞书、钉钉、Webhook 和网页组件，把大模型能力放到员工和系统每天都会用到的入口里。
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

如果你是第一次试用，建议先按这篇教程走完整闭环：[10 分钟部署 NeoGate](docs/quickstart-10-minutes.zh.md)。

### 🐳 Docker 安装

Docker 安装是最快的部署方式，不需要在宿主机单独安装 Rust、Node.js 或 pnpm。需要多副本和横向扩展的生产环境，请查看：[集群部署文档](docs/deployment/cluster.zh.md)。

#### 单机部署

单机部署适合试用、内部部署和中小规模场景。Compose 会同时启动前端 Nginx、后端和 PostgreSQL，不需要额外准备 PostgreSQL 或 Redis。

```bash
# 使用预构建镜像，无需在服务器上编译
docker compose up -d
```

如果希望从源码本地构建镜像，或需要使用中国大陆镜像源构建，可以使用：

```bash
# 海外（Docker Hub 可直接访问）
docker compose -f docker-compose.build.yml up -d --build

# 中国大陆（使用国内镜像源）
docker compose -f docker-compose.cn.yml up -d --build
```

启动后访问 `http://服务器IP:8080`，首次运行向导会引导你完成管理员、服务模式、初始上游、价格、SMTP 和支付等配置。

#### 绑定域名和宿主机 Nginx

使用 Docker Compose 安装时，Compose 默认会把服务暴露到宿主机 `8080` 端口。宿主机 Nginx 可直接反向代理到 `http://127.0.0.1:8080`：

```bash
sudo cp deploy/nginx/docker-compose.conf.example /etc/nginx/conf.d/neogate.conf
sudo vim /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

#### 检查运行状态

部署完成后，可以先查看容器是否都处于 `running` 或 `healthy` 状态：

```bash
docker compose ps
```

通常会看到 `postgres`、`backend`、`web` 三个服务。若服务状态不是 `running`/`healthy`，可以查看日志定位原因：

```bash
docker compose logs -f
```

也可以只查看某个服务的日志，例如单机部署的后端：

```bash
docker compose logs -f backend
```

最后在浏览器访问 `http://服务器IP:8080`。如果可以打开首次运行向导或登录页面，通常表示前端、后端和反向代理链路已经正常。

#### 管理员密码恢复

如果管理员忘记密码或账号被锁定，可以在后端容器中重置：

```bash
docker compose exec backend neogate admin reset-password --username admin
```

如果管理员用户名不是 `admin`，请替换 `--username`。

### 🧑‍💻 源码本地运行

源码运行适合二次开发、调试和自定义部署。请先准备 PostgreSQL 16、Rust 1.94+、Node.js 20+ 和 pnpm。

#### 准备数据库

进入 PostgreSQL：

```bash
sudo -u postgres psql
```

```sql
CREATE USER neogate WITH PASSWORD 'change-me';
CREATE DATABASE neogate OWNER neogate;
\q
```

数据库连接地址：

```text
postgres://neogate:change-me@localhost:5432/neogate
```

#### 开发运行

分别启动后端、定时任务和前端：

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

打开 `http://localhost:5173`；远程开发时使用 `http://服务器IP:5173`。按照首次运行向导完成数据库连接、管理员账号、服务模式和初始上游配置。保存运行配置后如提示重启，重新启动后端即可。

#### 正式运行

构建后端、定时任务和前端：

```bash
cargo build --release -p neogate -p neogate-scheduler
cd frontend
pnpm install
pnpm build
cd ..
```

分别启动后端和定时任务：

```bash
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

```bash
./target/release/neogate-scheduler
```

正式环境应使用 systemd、supervisord 等工具保持进程常驻，并通过 Nginx 托管 `frontend/dist`、代理后端接口和配置 HTTPS。生产环境仍建议优先使用 Docker Compose 或集群部署方案。

---

## ✅ 生产建议

上线前至少确认：

- **安全与域名**：使用强管理员密码和随机生成的 `ADMIN_TOKEN_SECRET`、`UPSTREAM_SECRET_KEY`；通过 HTTPS 对外提供服务，并正确配置 `PUBLIC_BASE_URL` 和反向代理。
- **上游可用性**：通过通道诊断验证模型、端点和凭证，保持定时任务进程运行，以执行通道探测和模型目录同步。
- **计费配置**：计费模式上线前检查模型价格、额度策略、充值套餐和支付回调；内部模式也应确认用量与成本记录正常。
- **数据持久化**：持久化并定期备份 PostgreSQL、运行配置和系统密钥；使用后台图片生成或其他需要本地响应资产的任务时，同时持久化 `NEOGATE_ASSET_DIR`。
- **集群部署**：多副本部署需要共享 PostgreSQL、Redis 和系统密钥，并根据实际负载规划 API、Worker 与 Scheduler 角色。

更多配置和容量限制请参考：

- [单机部署](docs/deployment/standalone.zh.md)
- [集群部署](docs/deployment/cluster.zh.md)

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

<table>
  <tr>
    <th>微信联系</th>
    <th>官方 QQ 群</th>
  </tr>
  <tr>
    <td><img src="docs/assets/wechat.png" alt="NeoGate 微信联系二维码" width="220" /></td>
    <td><img src="docs/assets/qq.png" alt="NeoGate 官方 QQ 群二维码" width="220" /></td>
  </tr>
</table>
