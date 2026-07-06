# 10 分钟部署 NeoGate

这是一份最短路径教程，适合第一次用 Docker Compose 试用 NeoGate。更完整的部署说明请看[单机部署](deployment/standalone.zh.md)和[集群部署](deployment/cluster.zh.md)。

## 准备

你需要：

- 一台可以运行 Docker 和 Docker Compose 的机器。
- 一个可用的上游模型 API Key。
- 服务器防火墙或安全组放行 `8080` 端口。

确认 Docker 可用：

```bash
docker --version
docker compose version
```

## 1. 启动服务

在 NeoGate 仓库根目录执行：

```bash
docker compose up -d
```

这个命令会启动 PostgreSQL、后端和前端 Nginx。

检查容器状态：

```bash
docker compose ps
```

正常情况下应看到 `postgres`、`backend`、`web` 处于 `running` 或 `healthy` 状态。

查看日志：

```bash
docker compose logs -f
```

只查看后端日志：

```bash
docker compose logs -f backend
```

## 2. 可选：绑定域名和宿主机 Nginx

Docker Compose 默认把 NeoGate 暴露到宿主机 `8080` 端口。直接通过 `http://服务器IP:8080` 访问时，可以跳过这一步。

如果你希望绑定域名，可以让宿主机 Nginx 反向代理到：

```text
http://127.0.0.1:8080
```

仓库提供了示例配置：

```bash
sudo cp deploy/nginx/docker-compose.conf.example /etc/nginx/conf.d/neogate.conf
sudo vim /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

修改配置时，重点确认域名、证书路径和反代目标地址。

## 3. 完成首次运行向导

浏览器访问：

```text
http://服务器IP:8080
```

本机试用可以访问：

```text
http://127.0.0.1:8080
```

首次启动会进入初始化向导。按页面提示完成：

- 管理员账号。
- 服务模式。
- 站点名称和公开访问地址。
- 初始上游 provider、API Key、模型和价格。
- 可选 SMTP 和支付设置。

如果只是内部试用，建议先选择内部模式。内部模式默认不要求用户或项目有可用余额即可调用。

## 4. 检查上游通道

进入后台后，打开：

```text
管理后台 -> 上游服务
```

确认至少有一个启用的通道，并检查：

- Base URL 是否正确。
- API Key 是否可用。
- 模型列表是否包含准备调用的模型。
- 通道诊断是否通过。

如果诊断失败，优先检查 API Key、模型名、Base URL、服务器网络和 provider 协议兼容性。

## 5. 创建或复制 API Key

内部模式下，推荐通过项目来管理团队或业务应用：

```text
管理后台 -> 项目管理
```

你可以创建一个项目，例如：

```text
Internal AI Gateway
```

然后为项目成员创建或查看 API Key。也可以进入：

```text
用户中心 -> API Key
```

复制一个可用的 NeoGate API Key。业务系统后续使用这个 Key 调用 NeoGate，而不是直接使用上游 provider 的 Key。

## 6. 发送测试请求

把 `YOUR_NEOGATE_API_KEY` 和 `MODEL_NAME` 替换成你的 NeoGate API Key 和模型名：

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "MODEL_NAME",
    "messages": [
      {
        "role": "user",
        "content": "用一句话介绍 NeoGate。"
      }
    ]
  }'
```

如果 NeoGate 部署在远程服务器，把地址改成：

```text
http://服务器IP:8080/v1/chat/completions
```

成功时会收到 OpenAI-compatible 格式的响应。

## 7. 可选：自动配置本机工具

如果你想让本机的 Codex 或 Claude Code 直接走 NeoGate，可以使用 NeoGate 提供的自动配置脚本。

Linux / macOS / WSL：

```bash
curl -fsSL http://服务器IP:8080/install | bash
```

Windows PowerShell：

```powershell
irm http://服务器IP:8080/install.ps1 | iex
```

如果已经绑定域名，把 `http://服务器IP:8080` 替换成你的 NeoGate 访问地址。

脚本会按提示完成：

- 验证 NeoGate API Key。
- 选择要配置的客户端，例如 Codex CLI 或 Claude Code。
- 从可用模型列表中选择模型。
- 展示配置摘要。
- 写入 Base URL、API Key 和模型名。
- 执行一次网关转发测试。

如果本机已经配置过 NeoGate，再次运行同一条命令会尝试读取上次的 API Key、模型和客户端，并提示你切换模型、更换 API Key 或重新安装。
