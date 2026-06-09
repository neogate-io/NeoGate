<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy, UserFilled } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { useAuthStore } from '../../stores/auth'

const githubUrl = 'https://github.com/neogate-io/NeoGate'
const auth = useAuthStore()
const { locale, t } = useLocale()
const dashboardLink = computed(() => (auth.isAdmin ? '/admin' : '/home/overview'))
const siteOrigin = computed(() => window.location.origin)
const openAiBaseUrl = computed(() => `${siteOrigin.value}/v1`)
const anthropicBaseUrl = computed(() => `${siteOrigin.value}/anthropic`)

function scrollToSection(id: string) {
  document.getElementById(id)?.scrollIntoView({
    behavior: 'smooth',
    block: 'start'
  })
}

async function copyDocText(text: string) {
  await navigator.clipboard.writeText(text)
  ElMessage.success(t('apiKeyCopied'))
}

const openAiModelsCurl = computed(
  () => `curl ${openAiBaseUrl.value}/models \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const chatCurl = computed(
  () => `curl ${openAiBaseUrl.value}/chat/completions \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [
      { "role": "user", "content": "用一句话介绍 NeoGate" }
    ],
    "stream": false
  }'`
)

const responsesCurl = computed(
  () => `curl ${openAiBaseUrl.value}/responses \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "写一个 TypeScript 防抖函数",
    "stream": false
  }'`
)

const anthropicCurl = computed(
  () => `curl ${anthropicBaseUrl.value}/v1/messages \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 1024,
    "messages": [
      { "role": "user", "content": "用一句话介绍 NeoGate" }
    ],
    "stream": false
  }'`
)

const pythonExample = computed(
  () => `from openai import OpenAI

client = OpenAI(
    api_key="YOUR_NEOGATE_API_KEY",
    base_url="${openAiBaseUrl.value}",
)

response = client.chat.completions.create(
    model="gpt-5.5",
    messages=[
        {"role": "user", "content": "用一句话介绍 NeoGate"}
    ],
)

print(response.choices[0].message.content)`
)

const nodeExample = computed(
  () => `import OpenAI from "openai";

const client = new OpenAI({
  apiKey: "YOUR_NEOGATE_API_KEY",
  baseURL: "${openAiBaseUrl.value}",
});

const response = await client.responses.create({
  model: "gpt-5.5",
  input: "写一个 TypeScript 防抖函数",
});

console.log(response.output_text);`
)

const streamExample = computed(
  () => `curl ${openAiBaseUrl.value}/chat/completions \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [
      { "role": "user", "content": "连续输出 3 个要点" }
    ],
    "stream": true
  }'`
)

const errorExample = `{
  "error": {
    "message": "insufficient credit",
    "type": "invalid_request_error"
  }
}`

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      title: '接口文档',
      subtitle:
        'NeoGate 对外提供 OpenAI / Anthropic 兼容接口。下游应用只需要使用 NeoGate Base URL 和自己的 API Key，上游供应商密钥由后台统一托管。',
      menuTitle: '目录',
      menu: [
        ['overview', '概览', '1. 概览'],
        ['auth', '认证', '2. 认证与 Base URL'],
        ['endpoints', '接口列表', '3. 接口列表'],
        ['openai', 'OpenAI 兼容', '4. OpenAI 兼容接口'],
        ['anthropic', 'Anthropic 兼容', '5. Anthropic 兼容接口'],
        ['sdk', 'SDK 示例', '6. SDK 示例'],
        ['streaming', '流式响应', '7. 流式响应'],
        ['errors', '错误与计费', '8. 错误与计费']
      ],
      overviewIntro:
        '接口会根据请求中的 model 自动选择可用上游服务，并记录用量、Token、费用、首字延迟和总延迟。',
      overviewItems: [
        ['协议兼容', '支持 OpenAI Chat Completions、Responses，以及 Anthropic Messages。'],
        ['统一认证', '所有请求使用 NeoGate API Key；后台负责上游 Key 和渠道路由。'],
        ['用量审计', '成功和失败请求都会进入用量记录，便于用户和管理员排查。'],
        ['额度控制', '收费或额度校验模式下，余额不足会拒绝调用。']
      ],
      authIntro: '请先在用户后台创建 API 密钥，或从公开首页领取密钥。',
      authItems: [
        ['OpenAI Base URL', openAiBaseUrl.value],
        ['Anthropic Base URL', anthropicBaseUrl.value],
        ['OpenAI 认证头', 'Authorization: Bearer YOUR_NEOGATE_API_KEY'],
        ['Anthropic 认证头', 'x-api-key: YOUR_NEOGATE_API_KEY']
      ],
      endpointsIntro: '以下为当前网关公开的大模型调用接口。',
      endpointHeaders: ['接口', '方法', '路径', '说明'],
      endpoints: [
        ['模型列表', 'GET', '/v1/models', '返回当前 API Key 可调用的 OpenAI 协议模型。'],
        ['Chat Completions', 'POST', '/v1/chat/completions', '兼容 OpenAI 对话补全。'],
        ['Responses', 'POST', '/v1/responses', '兼容 OpenAI Responses。'],
        ['Responses 详情', 'GET', '/v1/responses/{response_id}', '查询异步 Responses 状态。'],
        ['Responses 取消', 'POST', '/v1/responses/{response_id}/cancel', '取消异步 Responses。'],
        [
          'Anthropic 模型列表',
          'GET',
          '/anthropic/v1/messages/models',
          '返回当前 API Key 可调用的 Anthropic 协议模型。'
        ],
        ['Messages', 'POST', '/anthropic/v1/messages', '兼容 Anthropic Messages。'],
        ['Messages', 'POST', '/v1/messages', '兼容 Anthropic 原始路径。'],
        ['Message Batches', 'POST/GET', '/v1/messages/batches', '创建或查询批量任务。']
      ],
      modelListTitle: '获取模型列表',
      chatTitle: 'Chat Completions',
      chatText:
        '请求体遵循 OpenAI Chat Completions 格式。常用字段包括 model、messages、temperature、max_tokens 和 stream。',
      responsesTitle: 'Responses',
      responsesText:
        '请求体遵循 OpenAI Responses 格式。后台异步 Responses 需要 store=true；创建时不支持直接流式返回后台任务。',
      anthropicTitle: 'Messages',
      anthropicText:
        '请求体遵循 Anthropic Messages 格式。可传入 anthropic-version 与 anthropic-beta 请求头；未传版本时使用服务端默认版本。',
      sdkIntro: 'OpenAI 兼容 SDK 只需要把 base_url / baseURL 指向 NeoGate。',
      pythonTitle: 'Python',
      nodeTitle: 'Node.js',
      streamingText:
        '将 stream 设置为 true 即可获得 text/event-stream 响应。网关会透传上游流式数据，并在流结束后结算用量。',
      errorsText:
        '错误响应为 JSON。常见原因包括 API Key 无效、余额不足、模型未配置、上游服务不可用或上游返回失败。',
      errorTitle: '错误格式',
      billingNotes: [
        ['余额不足', '请求会在转发前被拒绝，不会消耗上游额度。'],
        ['模型未配置', '请确认后台渠道包含请求中的 model，且对应渠道已启用。'],
        ['上游失败', '网关会记录失败摘要，管理员可在用量或渠道健康状态中排查。']
      ]
    }
  }

  return {
    title: 'API Reference',
    subtitle:
      'NeoGate exposes OpenAI / Anthropic-compatible APIs. Client apps use the NeoGate Base URL and their own API key while upstream credentials stay managed in the admin console.',
    menuTitle: 'Contents',
    menu: [
      ['overview', 'Overview', '1. Overview'],
      ['auth', 'Auth', '2. Auth and Base URL'],
      ['endpoints', 'Endpoints', '3. Endpoints'],
      ['openai', 'OpenAI Compatible', '4. OpenAI-compatible APIs'],
      ['anthropic', 'Anthropic Compatible', '5. Anthropic-compatible APIs'],
      ['sdk', 'SDK Examples', '6. SDK Examples'],
      ['streaming', 'Streaming', '7. Streaming'],
      ['errors', 'Errors and Billing', '8. Errors and Billing']
    ],
    overviewIntro:
      'The gateway routes by the requested model and records usage, tokens, cost, first-token latency, and total latency.',
    overviewItems: [
      [
        'Compatible protocols',
        'OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.'
      ],
      [
        'Unified auth',
        'Every request uses a NeoGate API key; upstream keys and routing stay server-side.'
      ],
      ['Usage audit', 'Successful and failed requests are recorded for user and admin review.'],
      ['Credit control', 'In paid or credit-required mode, insufficient balance rejects requests.']
    ],
    authIntro: 'Create an API key in the user console, or request one from the public home page.',
    authItems: [
      ['OpenAI Base URL', openAiBaseUrl.value],
      ['Anthropic Base URL', anthropicBaseUrl.value],
      ['OpenAI auth header', 'Authorization: Bearer YOUR_NEOGATE_API_KEY'],
      ['Anthropic auth header', 'x-api-key: YOUR_NEOGATE_API_KEY']
    ],
    endpointsIntro: 'These are the public model invocation endpoints exposed by this gateway.',
    endpointHeaders: ['API', 'Method', 'Path', 'Description'],
    endpoints: [
      ['Models', 'GET', '/v1/models', 'Lists OpenAI-protocol models available to the API key.'],
      ['Chat Completions', 'POST', '/v1/chat/completions', 'OpenAI-compatible chat completions.'],
      ['Responses', 'POST', '/v1/responses', 'OpenAI-compatible Responses.'],
      [
        'Response details',
        'GET',
        '/v1/responses/{response_id}',
        'Retrieves async Response status.'
      ],
      [
        'Cancel response',
        'POST',
        '/v1/responses/{response_id}/cancel',
        'Cancels an async Response.'
      ],
      [
        'Anthropic models',
        'GET',
        '/anthropic/v1/messages/models',
        'Lists Anthropic-protocol models available to the API key.'
      ],
      ['Messages', 'POST', '/anthropic/v1/messages', 'Anthropic-compatible Messages.'],
      ['Messages', 'POST', '/v1/messages', 'Anthropic-compatible original path.'],
      ['Message Batches', 'POST/GET', '/v1/messages/batches', 'Creates or lists batch tasks.']
    ],
    modelListTitle: 'List models',
    chatTitle: 'Chat Completions',
    chatText:
      'The request body follows the OpenAI Chat Completions format. Common fields include model, messages, temperature, max_tokens, and stream.',
    responsesTitle: 'Responses',
    responsesText:
      'The request body follows the OpenAI Responses format. Background Responses require store=true; create-time streaming is not supported for background tasks.',
    anthropicTitle: 'Messages',
    anthropicText:
      'The request body follows the Anthropic Messages format. You may pass anthropic-version and anthropic-beta headers; if no version is sent, the server default is used.',
    sdkIntro: 'For OpenAI-compatible SDKs, point base_url / baseURL to NeoGate.',
    pythonTitle: 'Python',
    nodeTitle: 'Node.js',
    streamingText:
      'Set stream to true to receive a text/event-stream response. The gateway relays upstream stream events and settles usage after the stream finishes.',
    errorsText:
      'Error responses are JSON. Common causes include invalid API keys, insufficient balance, unconfigured models, unavailable upstream services, or upstream failures.',
    errorTitle: 'Error format',
    billingNotes: [
      [
        'Insufficient balance',
        'The request is rejected before forwarding and does not spend upstream quota.'
      ],
      [
        'Model not configured',
        'Confirm the admin channel includes the requested model and is enabled.'
      ],
      [
        'Upstream failure',
        'The gateway records a failure summary for usage and channel health review.'
      ]
    ]
  }
})
</script>

<template>
  <div class="docs-page interfaces-page">
    <header class="home-header docs-header">
      <RouterLink class="home-brand" to="/" :aria-label="t('appName')">
        <img class="home-brand-logo" src="/logo.svg" :alt="t('appName')" />
      </RouterLink>
      <nav class="home-nav" :aria-label="t('appName')">
        <RouterLink to="/">{{ t('home') }}</RouterLink>
        <RouterLink to="/docs">{{ t('docs') }}</RouterLink>
        <RouterLink to="/interfaces">{{ t('interfaces') }}</RouterLink>
      </nav>
      <div class="home-header-actions">
        <LocaleToggleButton class="home-language-button" />
        <a
          class="github-link"
          :href="githubUrl"
          target="_blank"
          rel="noreferrer"
          :aria-label="t('github')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M12 2C6.48 2 2 6.58 2 12.26c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49 0-.24-.01-.88-.01-1.73-2.78.62-3.37-1.38-3.37-1.38-.45-1.19-1.11-1.5-1.11-1.5-.91-.64.07-.63.07-.63 1 .07 1.53 1.06 1.53 1.06.9 1.57 2.35 1.12 2.93.86.09-.67.35-1.12.63-1.38-2.22-.26-4.56-1.14-4.56-5.07 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.33 9.33 0 0 1 12 6.98c.85 0 1.7.12 2.5.34 1.9-1.33 2.74-1.05 2.74-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.94-2.34 4.81-4.57 5.07.36.32.68.95.68 1.92 0 1.38-.01 2.49-.01 2.83 0 .27.18.59.69.49A10.15 10.15 0 0 0 22 12.26C22 6.58 17.52 2 12 2Z"
            />
          </svg>
        </a>
        <RouterLink
          v-if="auth.isAuthed"
          class="home-login-link home-account-link"
          :to="dashboardLink"
          :aria-label="t('admin')"
        >
          <el-icon><UserFilled /></el-icon>
        </RouterLink>
        <RouterLink v-else class="home-login-link" to="/login">{{ t('signIn') }}</RouterLink>
      </div>
    </header>

    <main class="docs-main">
      <section class="docs-hero">
        <h1>{{ content.title }}</h1>
        <p>{{ content.subtitle }}</p>
      </section>

      <div class="docs-layout">
        <aside class="docs-sidebar">
          <h2>{{ content.menuTitle }}</h2>
          <nav>
            <a
              v-for="[id, label] in content.menu"
              :key="id"
              href=""
              @click.prevent="scrollToSection(id)"
            >
              {{ label }}
            </a>
          </nav>
        </aside>

        <div class="docs-content">
          <section id="overview" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[0][2] }}</h2>
              <p>{{ content.overviewIntro }}</p>
            </div>
            <div class="docs-feature-grid">
              <article
                v-for="[title, text] in content.overviewItems"
                :key="title"
                class="docs-feature"
              >
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>

          <section id="auth" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[1][2] }}</h2>
              <p>{{ content.authIntro }}</p>
            </div>
            <div class="interface-meta-grid">
              <article v-for="[label, value] in content.authItems" :key="label">
                <span>{{ label }}</span>
                <code>{{ value }}</code>
              </article>
            </div>
          </section>

          <section id="endpoints" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[2][2] }}</h2>
              <p>{{ content.endpointsIntro }}</p>
            </div>
            <div class="interface-table-wrap">
              <table class="interface-table">
                <thead>
                  <tr>
                    <th v-for="header in content.endpointHeaders" :key="header">{{ header }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="[name, method, path, description] in content.endpoints" :key="path">
                    <td>{{ name }}</td>
                    <td>
                      <span class="interface-method">{{ method }}</span>
                    </td>
                    <td>
                      <code>{{ path }}</code>
                    </td>
                    <td>{{ description }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>

          <section id="openai" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[3][2] }}</h2>
            </div>
            <div class="docs-guide-flow">
              <article class="docs-step-card">
                <h3>{{ content.modelListTitle }}</h3>
                <div class="docs-copy-block">
                  <el-button
                    :icon="DocumentCopy"
                    text
                    :aria-label="t('copy')"
                    @click="copyDocText(openAiModelsCurl)"
                  />
                  <pre
                    class="docs-code-sample docs-inner-code"
                  ><code>{{ openAiModelsCurl }}</code></pre>
                </div>
              </article>
              <article class="docs-step-card">
                <h3>{{ content.chatTitle }}</h3>
                <p>{{ content.chatText }}</p>
                <div class="docs-copy-block">
                  <el-button
                    :icon="DocumentCopy"
                    text
                    :aria-label="t('copy')"
                    @click="copyDocText(chatCurl)"
                  />
                  <pre class="docs-code-sample docs-inner-code"><code>{{ chatCurl }}</code></pre>
                </div>
              </article>
              <article class="docs-step-card">
                <h3>{{ content.responsesTitle }}</h3>
                <p>{{ content.responsesText }}</p>
                <div class="docs-copy-block">
                  <el-button
                    :icon="DocumentCopy"
                    text
                    :aria-label="t('copy')"
                    @click="copyDocText(responsesCurl)"
                  />
                  <pre
                    class="docs-code-sample docs-inner-code"
                  ><code>{{ responsesCurl }}</code></pre>
                </div>
              </article>
            </div>
          </section>

          <section id="anthropic" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[4][2] }}</h2>
              <p>{{ content.anthropicText }}</p>
            </div>
            <article class="docs-step-card">
              <h3>{{ content.anthropicTitle }}</h3>
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(anthropicCurl)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ anthropicCurl }}</code></pre>
              </div>
            </article>
          </section>

          <section id="sdk" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[5][2] }}</h2>
              <p>{{ content.sdkIntro }}</p>
            </div>
            <div class="docs-guide-flow">
              <article class="docs-step-card">
                <h3>{{ content.pythonTitle }}</h3>
                <div class="docs-copy-block">
                  <el-button
                    :icon="DocumentCopy"
                    text
                    :aria-label="t('copy')"
                    @click="copyDocText(pythonExample)"
                  />
                  <pre
                    class="docs-code-sample docs-inner-code"
                  ><code>{{ pythonExample }}</code></pre>
                </div>
              </article>
              <article class="docs-step-card">
                <h3>{{ content.nodeTitle }}</h3>
                <div class="docs-copy-block">
                  <el-button
                    :icon="DocumentCopy"
                    text
                    :aria-label="t('copy')"
                    @click="copyDocText(nodeExample)"
                  />
                  <pre class="docs-code-sample docs-inner-code"><code>{{ nodeExample }}</code></pre>
                </div>
              </article>
            </div>
          </section>

          <section id="streaming" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[6][2] }}</h2>
              <p>{{ content.streamingText }}</p>
            </div>
            <article class="docs-step-card">
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(streamExample)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ streamExample }}</code></pre>
              </div>
            </article>
          </section>

          <section id="errors" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[7][2] }}</h2>
              <p>{{ content.errorsText }}</p>
            </div>
            <article class="docs-step-card">
              <h3>{{ content.errorTitle }}</h3>
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(errorExample)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ errorExample }}</code></pre>
              </div>
            </article>
            <div class="docs-check-list">
              <article
                v-for="[title, text] in content.billingNotes"
                :key="title"
                class="docs-check-item"
              >
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>
        </div>
      </div>
    </main>
  </div>
</template>
