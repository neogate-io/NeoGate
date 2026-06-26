# 10 分钟部署 NeoGate

这篇教程面向第一次试用 NeoGate 的用户。目标是在一台已安装 Docker 的服务器或本机上，快速跑起一个私有大模型 API 网关，并完成一次真实调用。

完成后你会得到：

- 一个可访问的 NeoGate 控制台。
- 一个已配置的上游通道。
- 一个项目或用户 API Key。
- 一次通过 `/v1/chat/completions` 的测试请求。
- 一条可在后台查看的用量记录。

## 适合场景

本教程适合：

- 个人或小团队评估 NeoGate。
- 公司内部先搭一个统一 AI API 入口。
- 在正式部署前快速验证上游 provider、模型和接口兼容性。

如果你需要多副本、外部 PostgreSQL、Redis 和生产级横向扩展，请参考[集群部署](deployment/cluster.zh.md)。

## 准备工作

你需要：

- 一台可以运行 Docker 和 Docker Compose 的机器。
- 一个可用的上游模型 API Key，例如 OpenAI-compatible 或 Anthropic-compatible 服务。
- 服务器防火墙允许访问 NeoGate 暴露的端口，默认是 `8080`。

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

这个命令会启动：

- `postgres`：PostgreSQL 数据库。
- `backend`：NeoGate 后端 API、worker 和 scheduler。
- `web`：前端页面和 Nginx 反向代理。

查看运行状态：

```bash
docker compose ps
```

正常情况下，三个服务应处于 `running` 或 `healthy` 状态。

如果需要查看日志：

```bash
docker compose logs -f
```

只看后端日志：

```bash
docker compose logs -f backend
```

## 2. 打开首次运行向导

浏览器访问：

```text
http://服务器IP:8080
```

本机试用可以访问：

```text
http://127.0.0.1:8080
```

首次启动时，NeoGate 会进入初始化向导。按页面提示完成：

- 管理员账号。
- 服务模式。
- 站点名称和公开访问地址。
- 初始上游 provider、API Key、模型和价格。
- 可选 SMTP 和支付设置。

如果只是内部试用，建议先选择内部模式。内部模式不会要求用户必须有可用余额才能调用，更适合公司内部网关、个人测试和小团队试用。

## 3. 配置上游通道

进入后台后，打开：

```text
管理后台 -> 上游通道
```

确认至少有一个启用的通道，并检查：

- Provider 是否正确。
- Base URL 是否对应你的上游服务。
- API Key 是否可用。
- 模型列表包含你准备调用的模型。
- 通道诊断可以成功访问上游。

如果你使用的是 OpenAI-compatible 服务，通常需要配置 OpenAI 协议端点和对应模型名。

如果你使用的是 Anthropic-compatible 服务，确认 Anthropic 协议端点启用，并使用对应的模型名。

## 4. 创建项目和 API Key

内部模式下，推荐通过项目来承载业务或团队：

```text
管理后台 -> 项目管理
```

创建一个项目，例如：

```text
Internal AI Gateway
```

然后为项目成员创建或查看 API Key。也可以在用户端进入：

```text
用户中心 -> API Key
```

复制一个可用的 API Key。后续请求会用这个 Key 调用 NeoGate，而不是直接暴露上游 provider 的 Key。

## 5. 发送测试请求

把下面命令中的 `YOUR_NEOGATE_API_KEY` 和 `MODEL_NAME` 替换成你的 NeoGate API Key 和模型名：

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

如果 NeoGate 部署在远程服务器，把地址替换成：

```text
http://服务器IP:8080/v1/chat/completions
```

成功时，你会收到 OpenAI-compatible 格式的响应。

## 6. 查看用量

调用完成后，进入：

```text
管理后台 -> 用量记录
```

你应该能看到刚才的请求记录，包括：

- 用户或项目。
- API Key。
- Provider 和模型。
- Token 用量。
- 成本。
- 状态码和错误信息。
- 上游路由链路。

也可以进入：

```text
管理后台 -> 用量统计
```

查看按日期、模型、通道等维度汇总的用量和成本。

## 常见问题

### 页面打不开

先检查容器状态：

```bash
docker compose ps
```

再查看日志：

```bash
docker compose logs -f backend
docker compose logs -f web
```

确认服务器防火墙、安全组或本机端口没有阻止 `8080`。

### 上游调用失败

进入 `管理后台 -> 上游通道`，打开通道诊断，重点检查：

- Base URL 是否正确。
- API Key 是否有效。
- 模型名是否和上游一致。
- 上游服务是否支持当前协议。
- 服务器是否能访问上游网络。

### 返回余额不足

如果你选择了计费模式，用户或项目需要有可用额度才能调用。试用阶段可以：

- 改用内部模式。
- 在后台为项目或用户调整额度。
- 配置充值套餐和支付通道后再测试计费流程。

### 流式请求或图片请求超时

长耗时请求需要确认反向代理和上游超时设置。Docker Compose 默认配置已经包含常用的流式和长请求设置。如果你自行接入宿主机 Nginx，请参考[单机部署文档](deployment/standalone.zh.md)中的 Nginx 配置建议。

## 下一步

完成这篇教程后，可以继续：

- 阅读[单机部署](deployment/standalone.zh.md)，了解常用运维命令。
- 阅读[集群部署](deployment/cluster.zh.md)，准备多副本生产环境。
