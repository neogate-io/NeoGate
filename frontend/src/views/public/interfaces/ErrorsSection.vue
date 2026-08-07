<script setup lang="ts">
import { computed } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import CodeSampleCard from './CodeSampleCard.vue'

const { locale } = useLocale()
const errorExample = `{
  "error": {
    "type": "insufficient_quota",
    "code": "insufficient_quota",
    "message": "insufficient credit: available=0.831460, required=1.234000"
  }
}`

const anthropicErrorExample = `{
  "type": "error",
  "error": {
    "type": "authentication_error",
    "message": "invalid x-api-key"
  }
}`

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      errorsTitle: '4. 错误码',
      errorIntro: '错误响应为 JSON。不同上游可能返回不同 message，网关会尽量保留可排查的信息。',
      errorOpenAiTitle: 'OpenAI 兼容格式',
      errorAnthropicTitle: 'Anthropic 兼容格式',
      errorHeaders: ['HTTP 状态', '常见原因', '建议处理'],
      errors: [
        [
          '400',
          '请求体格式错误、缺少字段或异步参数不满足要求',
          '检查 JSON、model、messages、store 等字段。'
        ],
        ['401', 'API Key 缺失或无效', '检查 Authorization 或 x-api-key 请求头。'],
        [
          '403',
          'API Key 停用、账号未启用、模型受限或余额不足',
          '确认用户状态、Key 状态和模型权限；余额不足请充值或联系管理员调整额度。'
        ],
        [
          '404/503',
          '模型没有可用渠道或上游服务不可用',
          '确认后台渠道模型、Key 健康状态和服务商可用性。'
        ],
        ['429/5xx', '上游限流、超时或返回失败', '稍后重试，或在后台切换/补充上游渠道。']
      ]
    }
  }

  return {
    errorsTitle: '4. Errors',
    errorIntro:
      'Error responses are JSON. Upstream providers may return different messages; the gateway keeps useful diagnostic information where possible.',
    errorOpenAiTitle: 'OpenAI-compatible format',
    errorAnthropicTitle: 'Anthropic-compatible format',
    errorHeaders: ['HTTP status', 'Common reason', 'Suggested handling'],
    errors: [
      [
        '400',
        'Invalid request body, missing fields, or invalid async parameters',
        'Check JSON, model, messages, store, and related fields.'
      ],
      ['401', 'Missing or invalid API key', 'Check the Authorization or x-api-key request header.'],
      [
        '403',
        'Disabled key, disabled account, model restriction, or insufficient balance',
        'Check user status, key status, and model permissions; if the balance is insufficient, recharge or ask an admin to adjust the quota.'
      ],
      [
        '404/503',
        'No available channel for the model or upstream unavailable',
        'Check channel models, key health, and provider availability.'
      ],
      [
        '429/5xx',
        'Upstream rate limit, timeout, or failure',
        'Retry later or add/switch upstream channels in the admin console.'
      ]
    ]
  }
})
</script>

<template>
  <section id="errors" class="docs-section">
    <div class="docs-section-heading">
      <h2>{{ content.errorsTitle }}</h2>
      <p>{{ content.errorIntro }}</p>
    </div>
    <CodeSampleCard :title="content.errorOpenAiTitle" :code="errorExample" :collapsible="false" />
    <CodeSampleCard
      :title="content.errorAnthropicTitle"
      :code="anthropicErrorExample"
      :collapsible="false"
    />
    <div class="interface-table-wrap">
      <table class="interface-table">
        <thead>
          <tr>
            <th v-for="header in content.errorHeaders" :key="header">{{ header }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="[status, reason, handling] in content.errors" :key="status">
            <td>
              <span class="interface-method">{{ status }}</span>
            </td>
            <td>{{ reason }}</td>
            <td>{{ handling }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
