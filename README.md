# NeoGate

[English](README.en.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)

NeoGate 是一个使用 Rust 构建的轻量级大模型 API 网关，目标是提供极致的请求转发性能，并保持简单易用。它帮助团队把多个模型供应商收拢到统一入口，集中管理访问密钥、模型路由和用量记录。

仓库地址：[neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

## 1. 功能概览

- 给团队、客户或内部应用分发独立 API key，不再直接暴露上游供应商密钥。
- 在一个后台里管理 OpenAI、Anthropic 等上游模型服务，并按模型、优先级和权重分配请求。
- 对外提供 OpenAI 兼容和 Anthropic 兼容接口，让现有客户端少改配置即可接入。
- 记录用户和 API key 的调用用量，方便排查问题、分析成本和后续计费。
- 支持首次运行时选择服务模式，既可作为团队内部网关使用，也可开启计费和支付能力面向用户收费。
- 在上游 key 失败时自动冷却并切换可用 key，减少单个密钥异常对服务的影响。

## 2. 服务模式

NeoGate 首次运行时需要选择团队内部模式或计费模式。两种模式都支持统一入口、上游密钥隐藏、模型路由和用量记录，主要区别在于是否要求用户先有额度、是否接入支付通道。

- 团队内部模式：适合公司、部门或项目组自用，也适合给内部应用、自动化脚本和成员分发 API key。默认不要求可用额度即可调用；系统仍会记录用量和费用，便于成本分析和内部管理。
- 计费模式：适合面向客户、开发者或外部用户提供收费模型调用服务。用户需要有可用额度后才能调用，并可通过支付通道充值；上线前需要配置模型价格、充值套餐和支付通道。

## 3. 快速开始

### 1. Docker 安装

Docker 安装分为单机部署和集群部署。两种方式都不需要在宿主机单独安装 Rust、Node.js 或 pnpm。

#### 单机部署

单机部署适合大多数起步场景。Compose 会同时启动前端 Nginx、后端和 PostgreSQL，不需要额外准备 PostgreSQL 或 Redis。

直接启动：

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

生产环境请替换 `.env.cluster` 中的默认密码、域名和共享密钥。单机部署缺失的后端密钥可由首次运行向导自动生成并写入后端配置卷。

### 2. 源码本地运行

源码本地运行适合开发、调试或从源码体验首次运行流程。正式部署建议优先使用上面的 Docker Compose；Compose 会先构建前端，再由 Nginx 托管静态文件。

源码本地运行需要先在服务器上准备这些依赖：

- PostgreSQL 16 或兼容版本
- Rust 1.85 或更新版本
- Node.js 20 或兼容版本
- pnpm

确认这些命令可用：

```bash
psql --version
cargo --version
node --version
pnpm --version
```

如果 `cargo --version` 显示的是 1.75 等较旧版本，请升级 Rust 工具链后再运行后端；旧版 Cargo 无法编译部分依赖。

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

启动后端：

```bash
cd backend
cargo run
```

后端默认监听：

```text
http://127.0.0.1:8080
```

在另一个终端进入项目根目录，启动前端：

```bash
cd frontend
pnpm install
pnpm dev --host 0.0.0.0
```

打开 `http://服务器IP:5173`，页面会自动跳转到首次运行向导。按页面提示完成：

- 运行配置：填写 PostgreSQL 连接、站点名称和公开访问地址。保存后如果提示需要重启，请重新运行后端并刷新页面。
- 管理员账号：创建管理员用户名和密码。
- 服务模式：选择团队内部模式或计费模式；计费模式可以同时配置支付通道。
- 初始上游：配置供应商、协议、Base URL、API key、模型和模型价格。
- SMTP：如需邮箱领取 API key 或密码重置，在向导中启用并填写 SMTP；也可以稍后在管理员后台配置。

向导完成后会进入登录页，使用刚创建的管理员账号登录。

前端开发服务会代理管理后台请求到后端。注意：`pnpm dev` 启动的是 Vite 开发服务，只适合本地开发和调试，不应作为生产服务。

正式从源码部署前端时，请执行：

```bash
cd frontend
pnpm install
pnpm build
```

然后将 `frontend/dist` 交给 Nginx 等静态 Web 服务托管，并将 `/api/`、`/v1/`、`/anthropic/`、`/readyz` 和 `/livez` 反向代理到后端。前端构建不需要指定后端公网地址。

## 4. 部署模式

NeoGate 可以按单节点或集群方式部署。大多数团队起步时使用单节点部署就够了：不需要 Redis，配置简单，部署和排障成本也更低。

- 单节点部署：默认模式，无需配置 `RUNTIME_MODE`，适合个人项目、小团队和早期生产环境。`docker-compose.yml` 会同时启动前端 Nginx、后端和 PostgreSQL。
- 集群部署：设置 `RUNTIME_MODE=distributed`，多个后端 API/worker 共享 PostgreSQL 和 Redis，适合明确需要多副本和横向扩展的场景。`docker-compose.cluster.yml` 不包含 PostgreSQL/Redis。

没有明确的多副本需求时，建议优先使用单节点部署。

## 5. 生产建议

上线前至少确认：

- 设置 `APP_ENV=production`。
- 在首次运行向导中创建管理员账号，不要使用弱密码。
- 使用足够长且随机的 `ADMIN_TOKEN_SECRET` 和 `UPSTREAM_SECRET_KEY`；单机部署可由首次运行向导生成，集群部署需要提前写入所有节点共享的环境配置。
- 在首次运行向导或环境配置中设置可信的 `PUBLIC_BASE_URL`，用于生成密码重置链接。
- 在首次运行向导或环境配置中设置 `SITE_NAME`，用于页面、邮件和支付网关显示。
- 如果前端与 API 是跨域访问，设置正确的 `CORS_ALLOWED_ORIGINS`；同域反向代理部署通常无需额外配置。
- 如需公开邮箱领取 API key，在首次运行向导或管理员后台的系统设置中配置 SMTP。
- 如需使用计费模式，在首次运行向导或管理员后台配置模型价格、充值套餐和支付通道。
- 如需集群部署，设置 `RUNTIME_MODE=distributed` 并配置 Redis；否则保持默认单节点模式即可。

## 6. 开源协议

NeoGate 社区版使用 [GNU Affero General Public License v3.0](LICENSE) 开源协议（`AGPL-3.0-only`）。

NeoGate 同时提供分层商业授权：普通企业内部使用可申请免费的书面 Internal Commercial License；客户交付、托管服务、SaaS、OEM、MSP、白标、转售等场景需要单独 Commercial License；企业版功能使用商业 EULA。详见 [LICENSING.md](LICENSING.md)。

NeoGate 名称、Logo 及相关标识不随 AGPL 授权，使用边界见 [TRADEMARKS.md](TRADEMARKS.md)。

## 7. 获取帮助

- 问题反馈：[GitHub Issues](https://github.com/neogate-io/NeoGate/issues)
- 代码贡献：[Pull Requests](https://github.com/neogate-io/NeoGate/pulls)
