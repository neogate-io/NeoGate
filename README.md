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
- 在上游 key 失败时自动冷却并切换可用 key，减少单个密钥异常对服务的影响。
- 支持用户通过邮箱自助领取 API key，适合搭建轻量的模型服务入口。
- 提供中英文管理界面，适合自部署社区版，也方便后续扩展企业管控能力。

## 2. 适用场景

- 团队内部已经在使用多个大模型供应商，希望统一入口和运维方式。
- 正在为客户、成员或内部系统提供模型调用能力，需要比直接共享上游 key 更可控的方案。
- 需要在内网、私有云或自有服务器中部署模型访问入口，保持密钥、用量和路由策略可控。
- 需要一个轻量网关作为 AI 应用、自动化工具、开发者平台或内部模型服务的基础设施。

## 3. 快速开始

### 1. 准备数据库

NeoGate 需要 PostgreSQL 16 或兼容版本。

```bash
createdb neogate
```

### 2. 启动后端

```bash
cp backend/.env.example backend/.env
```

编辑 `backend/.env`，至少确认这些配置：

```dotenv
DATABASE_URL=postgres://localhost/neogate
PUBLIC_BASE_URL=https://neogate.example.com
ADMIN_TOKEN_SECRET=change-me-admin-token-secret-in-production
UPSTREAM_SECRET_KEY=change-me-upstream-secret-key-in-production
```

首次启动后端时会在 `admin` 表为空时创建默认管理员 `admin` / `password`，后续管理员登录使用数据库中的密码哈希。

启动后端：

```bash
cd backend
cargo run
```

后端默认监听：

```text
http://127.0.0.1:8080
```

### 3. 启动前端

```bash
cd frontend
pnpm install
pnpm dev
```

前端开发服务会代理管理后台请求到后端。生产部署时，可以通过 `VITE_NEOGATE_BACKEND_ORIGIN` 指定后端公网地址。

### 4. Docker 启动

也可以使用 Docker Compose 启动：

```bash
cp backend/.env.example .env
docker compose up --build
```

使用 Docker Compose 时，请先在 `.env` 中取消注释并设置 `POSTGRES_PASSWORD`。生产环境请替换 `.env` 中的默认密钥。

## 4. 部署模式

NeoGate 可以按单节点或集群方式部署。大多数团队起步时使用单节点部署就够了：不需要 Redis，配置简单，部署和排障成本也更低。

- 单节点部署：默认模式，无需配置 `RUNTIME_MODE`，适合个人项目、小团队和早期生产环境。
- 集群部署：设置 `RUNTIME_MODE=distributed`，多个后端实例共享 Redis，适合明确需要多副本和横向扩展的场景。

没有明确的多副本需求时，建议优先使用单节点部署。

## 5. 生产建议

上线前至少确认：

- 设置 `APP_ENV=production`。
- 首次启动后替换默认管理员密码。
- 使用足够长且随机的 `ADMIN_TOKEN_SECRET` 和 `UPSTREAM_SECRET_KEY`。
- 设置可信的 `PUBLIC_BASE_URL`，用于生成密码重置链接。
- 如果前端与 API 是跨域访问，设置正确的 `CORS_ALLOWED_ORIGINS`；同域反向代理部署通常无需额外配置。
- 如需公开邮箱领取 API key，在管理员后台的系统设置中配置 SMTP。
- 如需集群部署，设置 `RUNTIME_MODE=distributed` 并配置 Redis；否则保持默认单节点模式即可。

## 6. 开源协议

NeoGate 社区版使用 [GNU Affero General Public License v3.0](LICENSE) 开源协议（`AGPL-3.0-only`）。

NeoGate 同时提供分层商业授权：普通企业内部使用可申请免费的书面 Internal Commercial License；客户交付、托管服务、SaaS、OEM、MSP、白标、转售等场景需要单独 Commercial License；企业版功能使用商业 EULA。详见 [LICENSING.md](LICENSING.md)。

NeoGate 名称、Logo 及相关标识不随 AGPL 授权，使用边界见 [TRADEMARKS.md](TRADEMARKS.md)。

## 7. 获取帮助

- 问题反馈：[GitHub Issues](https://github.com/neogate-io/NeoGate/issues)
- 代码贡献：[Pull Requests](https://github.com/neogate-io/NeoGate/pulls)
