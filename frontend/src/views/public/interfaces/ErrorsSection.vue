<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy } from '@element-plus/icons-vue'
import { useLocale } from '../../../composables/useLocale'
import { useCopyText } from '../../../composables/usePublicPage'

const { locale, t } = useLocale()
const copyDocText = useCopyText()
const errorExample = `{
  "error": {
    "message": "insufficient credit",
    "type": "invalid_request_error"
  }
}`

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      errorsTitle: '4. 错误码',
      errorIntro: '错误响应为 JSON。不同上游可能返回不同 message，网关会尽量保留可排查的信息。',
      errorHeaders: ['HTTP 状态', '常见原因', '建议处理'],
      errors: [
        ['401', 'API Key 缺失或无效', '检查 Authorization 或 x-api-key 请求头。'],
        ['403', 'API Key 停用、账号未启用或模型受限', '确认用户状态、Key 状态和模型权限。'],
        [
          '400',
          '请求体格式错误、缺少字段或异步参数不满足要求',
          '检查 JSON、model、messages、store 等字段。'
        ],
        ['402/400', '余额不足或额度校验未通过', '充值或联系管理员调整额度策略。'],
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
    errorHeaders: ['HTTP status', 'Common reason', 'Suggested handling'],
    errors: [
      ['401', 'Missing or invalid API key', 'Check the Authorization or x-api-key request header.'],
      [
        '403',
        'Disabled key, disabled account, or model restriction',
        'Check user status, key status, and model permissions.'
      ],
      [
        '400',
        'Invalid request body, missing fields, or invalid async parameters',
        'Check JSON, model, messages, store, and related fields.'
      ],
      [
        '402/400',
        'Insufficient balance or failed credit check',
        'Recharge or ask an admin to adjust the credit policy.'
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
    <article class="docs-step-card">
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
