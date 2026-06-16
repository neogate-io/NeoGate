# 集群部署文档

本文面向需要多副本、横向扩展或生产高可用部署的 NeoGate 环境。第一次体验或小规模自用建议优先使用主 README 中的单机 Docker Compose。

## 部署结构

集群版 Compose 使用 `docker-compose.cluster.yml`，包含这些服务：

| 服务 | 说明 |
| --- | --- |
| `web` | 前端 Nginx，负责提供前端静态资源并反向代理后端 API。 |
| `api` | 后端 API 进程，处理管理后台和模型转发请求。 |
| `worker` | 后端 worker 进程，处理后台任务、用量刷新等异步工作。 |
| `scheduler` | 定时任务进程，负责上游通道探测和模型目录同步。 |

集群版 Compose 不内置 PostgreSQL 和 Redis。API 和 worker 节点必须连接同一套外部 PostgreSQL、Redis；scheduler 必须连接同一套外部 PostgreSQL。所有角色都必须使用相同的共享密钥。

## 前置准备

上线前请准备：

- 外部 PostgreSQL，建议使用托管数据库或独立数据库实例。
- 外部 Redis，用于跨节点协调和共享状态。
- 稳定的公开访问地址，例如 `https://neogate.example.com`。
- 长随机共享密钥，所有 API、worker、scheduler 节点必须保持一致。
- 可访问的 Docker 和 Docker Compose 环境。

## 配置环境变量

复制集群环境变量示例：

```bash
cp deploy/env/cluster.env.example .env.cluster
```

至少需要检查并修改这些配置：

| 变量 | 说明 |
| --- | --- |
| `DATABASE_URL` | 外部 PostgreSQL 连接地址。 |
| `REDIS_URL` | 外部 Redis 连接地址。 |
| `REDIS_KEY_PREFIX` | Redis key 前缀，同一 Redis 被多个环境共用时应保持唯一。 |
| `PUBLIC_BASE_URL` | 对外访问地址，例如 `https://neogate.example.com`。 |
| `CORS_ALLOWED_ORIGINS` | 允许访问后端的前端来源，通常与 `PUBLIC_BASE_URL` 一致。 |
| `ADMIN_TOKEN_SECRET` | 管理员登录令牌签名密钥，必须是长随机字符串。 |
| `UPSTREAM_SECRET_KEY` | 上游凭证加密密钥，必须是长随机字符串。 |
| `WEB_PORT` | Docker Compose 直接暴露的前端端口，默认 `8080`。 |

> [!WARNING]
> 不要在生产环境使用示例文件中的默认密码和 `change-me` 密钥。`ADMIN_TOKEN_SECRET` 和 `UPSTREAM_SECRET_KEY` 一旦用于生产，应妥善备份；更换它们可能导致已有登录状态失效或已保存凭证无法解密。

## 启动集群

使用 `.env.cluster` 和集群 Compose 文件启动：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml up -d --build
```

这里需要同时传入两个参数：

- `--env-file .env.cluster`：让 Docker Compose 读取集群环境变量。
- `-f docker-compose.cluster.yml`：指定使用集群版 Compose 文件，而不是默认的单机 `docker-compose.yml`。

## 检查运行状态

查看集群服务状态：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml ps
```

正常情况下应看到 `web`、`api`、`worker`、`scheduler` 四个服务。其中 `web` 和 `api` 配置了 healthcheck，状态应逐步变为 `running` 或 `healthy`。

查看全部日志：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f
```

只查看 API 日志：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f api
```

只查看 worker 日志：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f worker
```

只查看 scheduler 日志：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f scheduler
```

最后访问 `PUBLIC_BASE_URL` 对应的地址。如果可以打开首次运行向导或登录页面，通常表示前端、后端和反向代理链路已经正常。

## 常用运维命令

停止集群：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml down
```

重新构建并启动：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml up -d --build
```

重启 API：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml restart api
```

重启定时任务：

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml restart scheduler
```

扩展 API 或 worker 副本时，可以结合外部负载均衡和编排系统进行。除非已经评估定时任务在生产拓扑下的行为，否则建议只保留一个 scheduler 实例。若直接使用 Docker Compose 多副本，请确保端口暴露、反向代理和共享存储策略符合生产环境要求。

## 故障排查

如果 `docker compose ps` 中服务没有进入 `running` 或 `healthy`，优先检查：

- `.env.cluster` 是否存在，并且 `PUBLIC_BASE_URL`、`DATABASE_URL`、`REDIS_URL` 已正确填写。
- PostgreSQL 和 Redis 是否允许当前服务器访问。
- `ADMIN_TOKEN_SECRET`、`UPSTREAM_SECRET_KEY` 是否为空或仍为示例值。
- `WEB_PORT` 是否被宿主机其他进程占用。
- `api` 日志中是否有数据库迁移、连接失败或配置缺失错误。
- 如果上游探测或模型目录同步没有执行，检查 `scheduler` 日志。

## 生产建议

- 使用 HTTPS 域名作为 `PUBLIC_BASE_URL`。
- PostgreSQL 和 Redis 开启访问控制、备份和监控。
- 将 `.env.cluster` 权限限制在部署用户可读范围内。
- 不要把生产 `.env.cluster` 提交到 Git 仓库。
- 在升级前备份数据库，并先在测试环境验证迁移流程。
