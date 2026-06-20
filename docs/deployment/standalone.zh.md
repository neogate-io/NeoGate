# 单机部署

单机部署适合评估、个人项目、小团队和早期生产环境。它不需要 Redis，部署和排障成本较低；需要多副本或横向扩展时，再切换到[集群部署](cluster.zh.md)。

## Docker Compose 部署

Docker Compose 是推荐的单机部署方式，不需要在宿主机单独安装 Rust、Node.js 或 pnpm。

```bash
docker compose up -d --build
```

它会启动：

- `web`：前端 Nginx，提供前端静态资源并反向代理后端 API。
- `backend`：后端 API、worker 和 scheduler。
- `postgres`：PostgreSQL 数据库。

启动后访问：

```text
http://服务器IP:8080
```

页面会进入首次运行向导。按提示完成管理员账号、服务模式、初始上游、价格、SMTP 和支付等配置。

### 检查运行状态

查看容器状态：

```bash
docker compose ps
```

正常情况下应看到 `postgres`、`backend`、`web` 三个服务处于 `running` 或 `healthy` 状态。

查看全部日志：

```bash
docker compose logs -f
```

只查看 backend 容器日志：

```bash
docker compose logs -f backend
```

如果浏览器可以打开首次运行向导或登录页面，通常表示前端、后端和反向代理链路已经正常。

### 常用命令

停止服务：

```bash
docker compose down
```

重新构建并启动：

```bash
docker compose up -d --build
```

重启 backend 容器：

```bash
docker compose restart backend
```

## 源码本地运行

源码运行分为开发部署和正式部署。开发部署适合调试或体验首次运行流程；正式部署适合希望从源码构建后自行托管进程和 Nginx 的场景。生产环境仍建议优先使用 Docker Compose。

### 公共准备

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

如果 `cargo --version` 显示的是 1.75 等较旧版本，请升级 Rust 工具链后再运行后端或定时任务。

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

首次启动时，如果运行配置还不完整，后端会进入 bootstrap 模式，并通过首次运行页面写入数据库连接、站点信息和随机密钥。通常不需要先手动编辑 `.env`。

### 开发部署

开发部署使用 Rust 调试构建和 Vite 开发服务。

| 服务 | 命令 | 默认地址 |
| --- | --- | --- |
| 后端 | `cargo run -p neogate` | `http://127.0.0.1:8080` |
| 定时任务 | `cargo run -p neogate-scheduler` | 无 HTTP 地址 |
| 前端 | `cd frontend && pnpm install && pnpm dev --host 0.0.0.0` | `http://服务器IP:5173` |

打开 `http://服务器IP:5173`，页面会自动跳转到首次运行向导。按提示完成运行配置、管理员账号、服务模式、初始上游和可选 SMTP；如果保存运行配置后提示需要重启，请重新运行后端并刷新页面。

### 正式部署

正式部署时建议使用 release 构建运行后端和定时任务，并用 Nginx 托管前端静态文件。后端和定时任务可交给 systemd、supervisord 或其他进程管理工具保持常驻。

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

也可以使用 systemd 托管两个进程。下面示例假设项目放在 `/opt/neogate`，并已在仓库根目录完成 release 构建：

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

建议为 NeoGate 日志添加 logrotate，避免日志文件长期运行后持续增长：

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
cd ..
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

## 故障排查

- `docker compose ps` 中服务未进入 `running` 或 `healthy`：先查看 `docker compose logs -f`。
- `backend` 无法启动：检查 PostgreSQL 是否健康、数据库连接地址是否正确。
- 页面打不开：确认宿主机 `8080` 端口没有被占用，并检查防火墙或安全组。
- 源码部署前端访问后端失败：确认 Nginx 反向代理指向 `http://127.0.0.1:8080`，并确认后端进程正在运行。
