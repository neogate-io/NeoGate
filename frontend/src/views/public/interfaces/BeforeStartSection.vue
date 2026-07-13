<script setup lang="ts">
import { computed } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import { useSiteBrand } from '../../../composables/useSiteBrand'

const { locale } = useLocale()
const { siteName } = useSiteBrand()
const siteOrigin = computed(() => window.location.origin)

const openAiBaseUrl = computed(() => `${siteOrigin.value}/v1`)

const anthropicBaseUrl = computed(() => `${siteOrigin.value}/anthropic`)

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      beforeTitle: '1. 接入前说明',
      beforeIntro: `下游应用只需要使用 ${siteName.value} Base URL 和自己的 ${siteName.value} API Key。上游供应商密钥、渠道健康状态和路由策略由后台统一管理。`,
      protocolCards: [
        [
          'OpenAI 兼容',
          openAiBaseUrl.value,
          '适合 OpenAI SDK、Chat Completions、Responses、Codex 与多数 OpenAI 生态工具。'
        ],
        [
          'Anthropic 兼容',
          anthropicBaseUrl.value,
          '适合 Claude SDK、Claude Code、Messages 与 Message Batches。'
        ]
      ],
      beforeItems: [
        ['API Key', '用户可在用户后台创建 API Key，也可在开放注册时从公开首页领取。'],
        ['模型路由', '请求中的 model 会匹配已启用上游服务；多个渠道命中时由网关选择可用渠道。'],
        ['权限与额度', 'API Key 被停用、用户余额不足或模型不在允许范围内时，请求会被拒绝。'],
        ['用量记录', '网关会记录模型、Token、费用、首字延迟、总延迟和失败摘要。']
      ]
    }
  }

  return {
    beforeTitle: '1. Before You Start',
    beforeIntro: `Client apps only need the ${siteName.value} Base URL and their ${siteName.value} API key. Upstream credentials, channel health, and routing are managed in the admin console.`,
    protocolCards: [
      [
        'OpenAI compatible',
        openAiBaseUrl.value,
        'For OpenAI SDKs, Chat Completions, Responses, Codex, and most OpenAI ecosystem tools.'
      ],
      [
        'Anthropic compatible',
        anthropicBaseUrl.value,
        'For Claude SDKs, Claude Code, Messages, and Message Batches.'
      ]
    ],
    beforeItems: [
      [
        'API key',
        'Users can create API keys in the user console or request one from the public home page when registration is open.'
      ],
      [
        'Model routing',
        'The requested model is matched against enabled upstream services; when multiple channels match, the gateway selects an available channel.'
      ],
      [
        'Permission and credit',
        'Disabled keys, insufficient balance, or disallowed models reject the request.'
      ],
      [
        'Usage records',
        'The gateway records model, tokens, cost, first-token latency, total latency, and failure summaries.'
      ]
    ]
  }
})
</script>

<template>
  <section id="before-start" class="docs-section">
    <div class="docs-section-heading">
      <h2>{{ content.beforeTitle }}</h2>
      <p>{{ content.beforeIntro }}</p>
    </div>
    <div class="docs-feature-grid">
      <article
        v-for="[title, baseUrl, text] in content.protocolCards"
        :key="title"
        class="docs-feature"
      >
        <h3>{{ title }}</h3>
        <code>{{ baseUrl }}</code>
        <p>{{ text }}</p>
      </article>
    </div>
    <div class="docs-check-list">
      <article v-for="[title, text] in content.beforeItems" :key="title" class="docs-check-item">
        <h3>{{ title }}</h3>
        <p>{{ text }}</p>
      </article>
    </div>
  </section>
</template>
