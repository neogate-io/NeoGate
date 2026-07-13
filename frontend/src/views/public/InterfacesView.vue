<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import PublicHeader from '../../components/common/PublicHeader.vue'
import BeforeStartSection from './interfaces/BeforeStartSection.vue'
import OpenAiSection from './interfaces/OpenAiSection.vue'
import AnthropicSection from './interfaces/AnthropicSection.vue'
import ErrorsSection from './interfaces/ErrorsSection.vue'
import BillingSection from './interfaces/BillingSection.vue'
import { useLocale } from '../../composables/useLocale'
import { useSiteBrand } from '../../composables/useSiteBrand'

const props = defineProps<{
  section?: 'before-start' | 'openai' | 'anthropic' | 'errors' | 'billing'
}>()

const { locale } = useLocale()
const { siteName } = useSiteBrand()
const route = useRoute()
const currentSection = computed(() => props.section ?? 'before-start')

const pageContent = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      title: '接口文档',
      subtitle: `${siteName.value} 按 OpenAI / Anthropic 官方 API 组织接口文档。已实现的接口可直接调用，其他官方接口会在状态列说明当前支持情况。`,
      menuTitle: '目录',
      menu: [
        ['before-start', '接入前说明', '1. 接入前说明'],
        ['openai', 'OpenAI 兼容接口', '2. OpenAI 兼容接口'],
        ['openai-quick-start', '快速开始', '2.1 快速开始', 'sub'],
        ['openai-text', '文本生成', '2.2 文本生成', 'sub'],
        ['openai-text-async', '文本生成（异步）', '2.3 文本生成（异步）', 'sub'],
        ['openai-images', '图片生成', '2.4 图片生成', 'sub'],
        ['openai-images-async', '图片生成（异步）', '2.5 图片生成（异步）', 'sub'],
        ['openai-videos', '视频生成', '2.6 视频生成', 'sub'],
        ['openai-embeddings', '向量嵌入', '2.7 向量嵌入', 'sub'],
        ['openai-models', '模型列表', '2.8 模型列表', 'sub'],
        ['openai-sdk', 'SDK 示例', '2.9 SDK 示例', 'sub'],
        ['anthropic', 'Anthropic 兼容接口', '3. Anthropic 兼容接口'],
        ['anthropic-quick-start', '快速开始', '3.1 快速开始', 'sub'],
        ['anthropic-text', '文本生成', '3.2 文本生成', 'sub'],
        ['anthropic-stream', '流式输出', '3.3 流式输出', 'sub'],
        ['anthropic-batches', '批量任务', '3.4 批量任务', 'sub'],
        ['anthropic-models', '模型列表', '3.5 模型列表', 'sub'],
        ['errors', '错误码', '4. 错误码'],
        ['billing', '计费与用量', '5. 计费与用量']
      ]
    }
  }

  return {
    title: 'API Reference',
    subtitle: `${siteName.value} follows the official OpenAI / Anthropic API structure. Implemented APIs are callable now; other official APIs show their current support status in the table.`,
    menuTitle: 'Contents',
    menu: [
      ['before-start', 'Before You Start', '1. Before You Start'],
      ['openai', 'OpenAI Compatible', '2. OpenAI-compatible APIs'],
      ['openai-quick-start', 'Quick start', '2.1 Quick start', 'sub'],
      ['openai-text', 'Text generation', '2.2 Text generation', 'sub'],
      ['openai-text-async', 'Text generation async', '2.3 Text generation async', 'sub'],
      ['openai-images', 'Images', '2.4 Images', 'sub'],
      ['openai-images-async', 'Images async', '2.5 Images async', 'sub'],
      ['openai-videos', 'Videos', '2.6 Videos', 'sub'],
      ['openai-embeddings', 'Embeddings', '2.7 Embeddings', 'sub'],
      ['openai-models', 'Models', '2.8 Models', 'sub'],
      ['openai-sdk', 'SDK examples', '2.9 SDK examples', 'sub'],
      ['anthropic', 'Anthropic Compatible', '3. Anthropic-compatible APIs'],
      ['anthropic-quick-start', 'Quick start', '3.1 Quick start', 'sub'],
      ['anthropic-text', 'Text generation', '3.2 Text generation', 'sub'],
      ['anthropic-stream', 'Streaming', '3.3 Streaming', 'sub'],
      ['anthropic-batches', 'Batch tasks', '3.4 Batch tasks', 'sub'],
      ['anthropic-models', 'Models', '3.5 Models', 'sub'],
      ['errors', 'Errors', '4. Errors'],
      ['billing', 'Billing and Usage', '5. Billing and Usage']
    ]
  }
})

const sectionRoutes = [
  ['before-start', '/interfaces/before-start'],
  ['openai', '/interfaces/openai'],
  ['openai-quick-start', '/interfaces/openai#openai-quick-start'],
  ['openai-text', '/interfaces/openai#openai-text'],
  ['openai-text-async', '/interfaces/openai#openai-text-async'],
  ['openai-images', '/interfaces/openai#openai-images'],
  ['openai-images-async', '/interfaces/openai#openai-images-async'],
  ['openai-videos', '/interfaces/openai#openai-videos'],
  ['openai-embeddings', '/interfaces/openai#openai-embeddings'],
  ['openai-models', '/interfaces/openai#openai-models'],
  ['openai-sdk', '/interfaces/openai#openai-sdk'],
  ['anthropic', '/interfaces/anthropic'],
  ['anthropic-quick-start', '/interfaces/anthropic#anthropic-quick-start'],
  ['anthropic-text', '/interfaces/anthropic#anthropic-text'],
  ['anthropic-stream', '/interfaces/anthropic#anthropic-stream'],
  ['anthropic-batches', '/interfaces/anthropic#anthropic-batches'],
  ['anthropic-models', '/interfaces/anthropic#anthropic-models'],
  ['errors', '/interfaces/errors'],
  ['billing', '/interfaces/billing']
] as const

function routeForSection(id: string) {
  return sectionRoutes.find(([sectionId]) => sectionId === id)?.[1] ?? '/interfaces/before-start'
}

function isSectionActive(id: string, level?: string) {
  const target = routeForSection(id)
  const [path, hash] = target.split('#')
  if (level === 'sub') return route.path === path && route.hash === `#${hash}`
  return route.path === path
}
</script>

<template>
  <div class="docs-page interfaces-page">
    <PublicHeader header-class="docs-header" />

    <main class="docs-main">
      <section class="docs-hero">
        <h1>{{ pageContent.title }}</h1>
        <p>{{ pageContent.subtitle }}</p>
      </section>

      <div class="docs-layout">
        <aside class="docs-sidebar">
          <h2>{{ pageContent.menuTitle }}</h2>
          <nav>
            <RouterLink
              v-for="[id, label, , level] in pageContent.menu"
              :key="id"
              :class="{
                'docs-sidebar-sub-link': level === 'sub',
                'docs-sidebar-active-link': isSectionActive(id, level)
              }"
              :to="routeForSection(id)"
            >
              {{ label }}
            </RouterLink>
          </nav>
        </aside>

        <div class="docs-content">
          <BeforeStartSection v-if="currentSection === 'before-start'" />
          <OpenAiSection v-else-if="currentSection === 'openai'" />
          <AnthropicSection v-else-if="currentSection === 'anthropic'" />
          <ErrorsSection v-else-if="currentSection === 'errors'" />
          <BillingSection v-else-if="currentSection === 'billing'" />
        </div>
      </div>
    </main>
  </div>
</template>
