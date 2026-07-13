<script setup lang="ts">
import { computed } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import { useSiteBrand } from '../../../composables/useSiteBrand'

const { locale } = useLocale()
const { siteName } = useSiteBrand()

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      billingTitle: '5. 计费与用量',
      billingIntro: `${siteName.value} 会在转发前预估额度，在响应完成后按实际 Token 和价格结算。`,
      billingItems: [
        ['文本生成', '按输入、输出以及缓存读写 Token 记录用量；价格由后台渠道价格与售价策略决定。'],
        ['流式请求', '流结束后统一结算；如果中途失败，会记录已知用量和失败摘要。'],
        ['异步/批量任务', '任务创建后会被跟踪，终态返回后结算用量。'],
        ['失败请求', '未转发到上游的请求通常不消耗上游额度，但会保留失败记录用于排查。']
      ]
    }
  }

  return {
    billingTitle: '5. Billing and Usage',
    billingIntro: `${siteName.value} estimates credit before forwarding and settles by actual tokens and configured prices after the response completes.`,
    billingItems: [
      [
        'Text generation',
        'Usage records include input, output, cache read, and cache write tokens. Prices come from channel prices and selling policy.'
      ],
      [
        'Streaming',
        'Usage is settled after the stream ends. If the stream fails, known usage and the failure summary are recorded.'
      ],
      [
        'Async and batch tasks',
        'Tasks are tracked after creation and settled when they reach a terminal state.'
      ],
      [
        'Failed requests',
        'Requests rejected before forwarding usually do not spend upstream quota, but failure records are kept for diagnostics.'
      ]
    ]
  }
})
</script>

<template>
  <section id="billing" class="docs-section">
    <div class="docs-section-heading">
      <h2>{{ content.billingTitle }}</h2>
      <p>{{ content.billingIntro }}</p>
    </div>
    <div class="docs-check-list">
      <article v-for="[title, text] in content.billingItems" :key="title" class="docs-check-item">
        <h3>{{ title }}</h3>
        <p>{{ text }}</p>
      </article>
    </div>
  </section>
</template>
