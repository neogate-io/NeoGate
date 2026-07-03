# NeoGate 应用接入说明

NeoGate 的「应用」用于把模型能力发布到外部入口。当前支持五类应用：

- 企业微信应用：使用企业微信自建应用回调。
- 飞书应用：使用飞书事件订阅回调。
- 钉钉应用：使用钉钉机器人或事件回调。
- Webhook 应用：内部系统通过 HTTP 调用。
- 网页组件应用：通过脚本嵌入网页。

## 公开入口

公开入口使用短路径，便于配置到外部平台：

```text
GET  /apps/wecom/{endpoint_id}/callback
POST /apps/wecom/{endpoint_id}/callback
POST /apps/feishu/{endpoint_id}/callback
POST /apps/dingtalk/{endpoint_id}/callback
POST /apps/webhook/{endpoint_id}
POST /apps/widget/{endpoint_id}/messages
GET  /widget/{endpoint_id}.js
```

管理接口仍保留在 `/api/admin/apps`，需要管理员登录。

## 部署要求

如果前端和后端通过 nginx 分离部署，需要确保 `/apps` 和 `/widget` 转发到后端 API 服务。示例：

```nginx
location ~ ^/(api|apps|widget|v1|anthropic|readyz|livez|install(?:\.ps1)?)(/|$) {
    proxy_pass http://127.0.0.1:8080;
}
```

企业微信后台配置回调 URL 时，使用：

```text
{PUBLIC_BASE_URL}/apps/wecom/{endpoint_id}/callback
```

飞书后台配置事件订阅 URL 时，使用：

```text
{PUBLIC_BASE_URL}/apps/feishu/{endpoint_id}/callback
```

钉钉后台配置回调 URL 时，使用：

```text
{PUBLIC_BASE_URL}/apps/dingtalk/{endpoint_id}/callback
```

`PUBLIC_BASE_URL` 应该是外部平台可访问的 NeoGate 后端公网地址。
