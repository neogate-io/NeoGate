# 魔力门（Moligate）

魔力门是一个基于 Rust/Axum 的轻量级大模型 API 网关，用于统一管理下游用户密钥、上游模型渠道、请求转发和基础用量记录。

## 功能概览

- 下游用户和 `user_key` 认证
- 上游 `channel` 管理，支持 OpenAI 和 Anthropic
- 每个渠道可配置多个 `channel_key`
- OpenAI 兼容接口和 Anthropic 兼容接口转发
- 按渠道优先级、权重、模型白名单和 key 选择策略调度
- 用量记录、上游 key 失败冷却和健康检查
- 公开邮箱领取 API key，支持中英文邮件品牌名
- Vue 管理前端，支持中文和英文界面

## 项目结构

```text
.
├── backend/          # Rust/Axum 后端
├── frontend/         # Vue 管理前端
├── migrations/       # SQLx 数据库迁移
├── docker-compose.yml
├── .env.example
└── README.md
```

## 快速开始

先创建本地 PostgreSQL 数据库：

```bash
createdb moligate
```

复制并编辑配置：

```bash
cp .env.example .env
```

至少需要确认这些配置：

```dotenv
DATABASE_URL=postgres://localhost/moligate
ADMIN_USERNAME=admin
ADMIN_PASSWORD=password
ADMIN_TOKEN_SECRET=change-me-admin-token-secret-in-production
UPSTREAM_SECRET_KEY=change-me-upstream-secret-key-in-production
MAIL_SMTP_HOST=smtp.example.com
MAIL_FROM_EMAIL=noreply@example.com
```

启动后端：

```bash
cd backend
cargo run
```

后端默认监听：

```text
http://127.0.0.1:8080
```

## 前端开发

前端接口默认使用同源请求。开发时 Vite 只代理管理后台需要的 `/api` 请求到后端；安装脚本里的 Codex/Claude 访问地址会直接指向 Rust 后端，不再经过 Vite 转发。

```bash
cd frontend
pnpm install
pnpm dev
```

生产部署前端时，使用 Vite 常规的环境变量文件配置后端公网域名。部署机本地可以复制模板到 `.env.production.local`：

```bash
cd frontend
cp .env.example .env.production.local
```

```dotenv
VITE_MOLIGATE_BACKEND_ORIGIN=https://api.example.com
```

生成的安装脚本会把 Codex/OpenAI base URL 配置为 `https://api.example.com/v1`，把 Claude/Anthropic base URL 配置为 `https://api.example.com/anthropic`。也可以在构建或启动 Vite 时直接传入 `VITE_MOLIGATE_BACKEND_ORIGIN=https://api.example.com`。

构建前端：

```bash
cd frontend
pnpm build
```

## Docker 启动

先准备 `.env`，并替换生产环境中的默认密码和密钥：

```bash
cp .env.example .env
docker compose up --build
```

`docker-compose.yml` 会启动：

- `postgres`: PostgreSQL 16
- `backend`: 魔力门后端服务

后端容器内监听 `0.0.0.0:8080`，宿主机端口由 `BACKEND_PORT` 控制，默认是 `8080`。

## 运行模式

魔力门只有两个运行模式：

- `RUNTIME_MODE=standalone`：不依赖 Redis，热余额、缓存失效和转发缓存都在单个进程内维护，适合单后端实例运行。
- `RUNTIME_MODE=distributed`：多个无状态后端实例共享 Redis，Redis 用于热余额和跨实例缓存失效。`REDIS_URL` 需要指向一个可写 Redis 入口，可以是单 Redis 主节点，也可以是由 Sentinel 维护并通过基础设施暴露出来的主节点入口；当前不支持 Redis Cluster 分片。

`PROCESS_ROLE` 控制进程承担的工作：

- `PROCESS_ROLE=all`：同时提供 HTTP 服务和后台账务/恢复 worker，默认值，适合单机和简单部署。
- `PROCESS_ROLE=api`：提供 HTTP 服务，写入 billing outbox，并 flush 本进程的轻量 usage 队列，但不扫描 durable billing backlog，也不运行 allocation recovery。
- `PROCESS_ROLE=worker`：只运行后台账务/恢复 worker，不监听 HTTP 端口。

分布式部署示例：

```dotenv
RUNTIME_MODE=distributed
REDIS_URL=redis://redis.example.com:6379/
REDIS_KEY_PREFIX=moligate
```

简单集群部署建议：负载均衡后面运行多个 `PROCESS_ROLE=api` 副本，并至少运行一个 `PROCESS_ROLE=worker` 副本处理 billing backlog 和 allocation recovery。

## 配置说明

`.env.example` 按用途分组：

- `App`: 运行环境、运行模式、监听地址和 CORS
- `Database`: 本地数据库和 Docker Compose 数据库配置
- `Admin`: 管理员登录和 token 签名密钥
- `Relay`: 上游协议默认参数和上游 key 加密密钥
- `Mail`: 公开领取 API key 的邮件发送配置
- `Advanced optional tuning`: 连接池、请求超时、key 冷却、转发缓存和后台用量写入

生产环境需要设置：

```dotenv
APP_ENV=production
RUNTIME_MODE=standalone
PROCESS_ROLE=all
CORS_ALLOWED_ORIGINS=https://admin.example.com,https://console.example.com
DB_POOL_MAX_CONNECTIONS=10
RELAY_BODY_LIMIT_BYTES=16777216
HTTP_POOL_MAX_IDLE_PER_HOST=100
HTTP_POOL_IDLE_TIMEOUT_SECONDS=90
USER_AUTH_CACHE_TTL_SECONDS=60
ROUTING_CACHE_TTL_SECONDS=30
PRICE_CACHE_TTL_SECONDS=300
USAGE_FLUSH_INTERVAL_MS=1000
USAGE_QUEUE_SIZE=8192
BILLING_OUTBOX_MAX_PENDING=10000
BILLING_OUTBOX_MAX_AGE_SECONDS=300
CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS=900
CREDIT_ALLOCATION_RECOVERY_INTERVAL_SECONDS=60
DEFAULT_OUTPUT_TOKENS=2048
```

生产模式会拒绝默认的管理员密码、默认签名密钥和默认上游 key 加密密钥，并要求 `ADMIN_TOKEN_SECRET` 和 `UPSTREAM_SECRET_KEY` 至少 32 个字符。

## 邮件和多语言品牌名

公开领取 API key 时，后端会根据请求中的 `locale` 选择邮件品牌名：

- 中文环境：`魔力门`
- 英文环境：`Moligate`

邮件主题示例：

- 中文：`魔力门 API 密钥`
- 英文：`Moligate API Key`

`MAIL_FROM_NAME` 和 `MAIL_SUBJECT_PREFIX` 可以留空。留空时后端会按语言自动选择品牌名；如果配置了它们，则使用配置值覆盖自动品牌名。

SMTP 配置示例：

```dotenv
MAIL_SMTP_HOST=smtp.example.com
MAIL_SMTP_PORT=587
MAIL_SMTP_USERNAME=apikey
MAIL_SMTP_PASSWORD=secret
MAIL_FROM_EMAIL=noreply@example.com
MAIL_FROM_NAME=
MAIL_SUBJECT_PREFIX=
```

`MAIL_SMTP_TLS` 默认是 `true`。端口 `465` 会使用 SMTPS，其它 TLS 端口会使用 STARTTLS。

## 数据模型

当前 PostgreSQL schema 保持精简：

- `"user"`: 网关用户
- `user_key`: 下游调用方使用的 API key
- `wallet`: 用户和 user key 的统一余额账户
- `provider`: 上游供应商目录，包含 OpenAI 兼容和 Anthropic 兼容两种协议入口的默认配置
- `channel`: 上游服务组，包含 provider、优先级、权重和 key 选择策略
- `channel_endpoint`: 上游服务组里的协议入口，包含协议格式、Base URL、模型白名单和健康状态
- `channel_key`: 上游服务组里的实际密钥
- `usage`: 转发用量记录
- `billing`: durable billing outbox
- `credit_allocation` 和 `credit_ledger`: 钱包预分配状态和不可变账本记录

`"user"` 在 SQL 中需要加引号，因为 `user` 是 PostgreSQL 关键字。

## 健康检查

```text
GET /healthz
GET /readyz
```

`/healthz` 用于进程存活检查，`/readyz` 会检查 PostgreSQL、billing outbox 写入健康和 backlog 阈值；当 `RUNTIME_MODE=distributed` 时，也会检查 Redis 可用性。

账务结算会先写入 durable `billing` outbox，并带短时间重试，然后由后台 worker 批量完成最终账本写入。非账务转发用量仍先进内存队列，再异步 flush 到 PostgreSQL；落库失败时，进程内批次会保留并重试；如果进程崩溃，尚未 flush 的非账务 usage 记录可能丢失。过久未结算的 active credit allocation 会按 `CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS` 周期性恢复；恢复前会先从当前运行模式的 hot-credit store 中移除对应 allocation。

## 管理接口

管理员登录：

```text
POST /api/login
```

登录成功后会返回 session token。管理接口使用：

```text
Authorization: Bearer <login_session_token>
```

常用管理接口：

```text
GET    /api/admin/health
GET    /api/admin/users
POST   /api/admin/users
PATCH  /api/admin/users/:id
DELETE /api/admin/users/:id

GET    /api/admin/user-keys
POST   /api/admin/user-keys
PATCH  /api/admin/user-keys/:id
DELETE /api/admin/user-keys/:id

GET    /api/admin/channels
POST   /api/admin/channels
PATCH  /api/admin/channels/:id
DELETE /api/admin/channels/:id

GET    /api/admin/channel-keys
GET    /api/admin/channels/:id/keys
POST   /api/admin/channels/:id/keys
PATCH  /api/admin/channels/:id/keys/:key_id
DELETE /api/admin/channels/:id/keys/:key_id

GET    /api/admin/usage
```

## 公开领取 API key

公开页面先创建 key 草稿：

```text
POST /api/user-key-drafts
```

然后用邮箱领取：

```text
POST /api/user-keys
```

请求体示例：

```json
{
  "email": "user@example.com",
  "draft_id": "<draft_id>",
  "locale": "zh-CN"
}
```

`locale` 可传 `zh-CN` 或 `en-US`。后端会据此选择邮件语言和品牌名。

## 转发接口

网关转发接口支持：

```text
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
```

下游调用方可以使用任一认证头：

```text
Authorization: Bearer <user_key>
x-api-key: <user_key>
```

OpenAI 兼容请求会转发为：

```text
Authorization: Bearer <解密后的上游 key>
```

Anthropic 兼容请求会转发为：

```text
x-api-key: <解密后的上游 key>
anthropic-version: <incoming value or ANTHROPIC_VERSION>
```

## 调度规则

每次转发请求时，魔力门会：

1. 校验下游 `user_key`
2. 按 provider、协议格式、模型白名单、启用状态、健康状态、冷却时间和父级服务组的可用 key 过滤协议入口
3. 选择优先级最高的一组渠道
4. 在同优先级渠道内按权重选择渠道
5. 按 `polling` 或 `random` 选择渠道 key
6. 上游失败时记录用量，并让失败 key 进入冷却

## 最小管理流程

1. 调用 `POST /api/login` 登录管理后台。
2. 创建用户：`POST /api/admin/users`。
3. 创建下游 key：`POST /api/admin/user-keys`。
4. 创建上游渠道：`POST /api/admin/channels`。
5. 添加上游密钥：`POST /api/admin/channels/:id/keys`。
6. 在价格页为已获取并启用的上游模型配置输入/输出价格。
7. 使用生成的下游 key 调用 `/v1/*` 转发接口。

## 验证

后端测试：

```bash
cd backend
cargo test
cargo check
```

前端类型检查和构建：

```bash
cd frontend
pnpm build
```

## 常见问题

### migration 1 was previously applied but has been modified

这是 SQLx 检测到已执行 migration 的 checksum 和当前文件不一致。开发环境如果数据库可以重建，可以清空本地库后重新运行迁移；生产环境不要直接修改已发布 migration，应新增 migration。

### 公开领取 API key 返回 500

优先检查 SMTP 配置。`MAIL_SMTP_TLS=true` 且端口为 `587` 时，后端会使用 STARTTLS；端口 `465` 会使用 SMTPS。服务端日志会记录 5xx 的底层错误链。
