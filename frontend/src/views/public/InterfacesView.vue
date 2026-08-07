<script setup lang="ts">
import { computed, ref, watch } from 'vue'
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
  sub?: string
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
        ['openai-videos', '视频与素材', '2.6 视频与素材', 'sub'],
        ['openai-audio', '音频转写', '2.7 音频转写', 'sub'],
        ['openai-embeddings', '向量嵌入', '2.8 向量嵌入', 'sub'],
        ['openai-models', '模型列表', '2.9 模型列表', 'sub'],
        ['openai-sdk', 'SDK 示例', '2.10 SDK 示例', 'sub'],
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
      ['openai-videos', 'Videos and assets', '2.6 Videos and assets', 'sub'],
      ['openai-audio', 'Audio transcription', '2.7 Audio transcription', 'sub'],
      ['openai-embeddings', 'Embeddings', '2.8 Embeddings', 'sub'],
      ['openai-models', 'Models', '2.9 Models', 'sub'],
      ['openai-sdk', 'SDK examples', '2.10 SDK examples', 'sub'],
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
  ['openai-quick-start', '/interfaces/openai/quick-start'],
  ['openai-text', '/interfaces/openai/text'],
  ['openai-text-async', '/interfaces/openai/text-async'],
  ['openai-images', '/interfaces/openai/images'],
  ['openai-images-async', '/interfaces/openai/images-async'],
  ['openai-videos', '/interfaces/openai/videos'],
  ['openai-audio', '/interfaces/openai/audio'],
  ['openai-embeddings', '/interfaces/openai/embeddings'],
  ['openai-models', '/interfaces/openai/models'],
  ['openai-sdk', '/interfaces/openai/sdk'],
  ['anthropic', '/interfaces/anthropic'],
  ['anthropic-quick-start', '/interfaces/anthropic/quick-start'],
  ['anthropic-text', '/interfaces/anthropic/text'],
  ['anthropic-stream', '/interfaces/anthropic/stream'],
  ['anthropic-batches', '/interfaces/anthropic/batches'],
  ['anthropic-models', '/interfaces/anthropic/models'],
  ['errors', '/interfaces/errors'],
  ['billing', '/interfaces/billing']
] as const

function routeForSection(id: string) {
  return sectionRoutes.find(([sectionId]) => sectionId === id)?.[1] ?? '/interfaces/before-start'
}

// Sidebar groups with sub entries (OpenAI / Anthropic) are collapsible. The
// group containing the current route starts expanded, the other collapsed.
const collapsedGroups = ref<Record<string, boolean>>({
  openai: !route.path.startsWith('/interfaces/openai'),
  anthropic: !route.path.startsWith('/interfaces/anthropic')
})

function toggleGroup(id: string) {
  collapsedGroups.value[id] = !collapsedGroups.value[id]
}

// Clicking the group title while already on its page toggles the sub entries
// instead of navigating again.
function onGroupClick(id: string, event: MouseEvent) {
  if (route.path === routeForSection(id)) {
    event.preventDefault()
    toggleGroup(id)
  }
}

watch(
  () => route.path,
  (path) => {
    if (path.startsWith('/interfaces/openai')) collapsedGroups.value.openai = false
    if (path.startsWith('/interfaces/anthropic')) collapsedGroups.value.anthropic = false
  }
)

const menuGroups = computed(() => {
  const groups: Array<{
    id: string
    label: string
    children: Array<{ id: string; label: string }>
  }> = []
  for (const [id, label, , level] of pageContent.value.menu) {
    if (level === 'sub') {
      groups[groups.length - 1]?.children.push({ id, label })
    } else {
      groups.push({ id, label, children: [] })
    }
  }
  return groups
})

function isSectionActive(id: string, level?: string) {
  const target = routeForSection(id)
  if (level === 'sub') return route.path === target
  // Group headers stay highlighted while on one of their sub pages.
  return route.path === target || route.path.startsWith(`${target}/`)
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
            <template v-for="group in menuGroups" :key="group.id">
              <div class="docs-sidebar-group">
                <RouterLink
                  :class="{ 'docs-sidebar-active-link': isSectionActive(group.id) }"
                  :to="routeForSection(group.id)"
                  @click="onGroupClick(group.id, $event)"
                >
                  {{ group.label }}
                </RouterLink>
                <button
                  v-if="group.children.length"
                  type="button"
                  class="docs-sidebar-group-toggle"
                  :aria-expanded="!collapsedGroups[group.id]"
                  :aria-label="group.label"
                  @click="toggleGroup(group.id)"
                />
              </div>
              <RouterLink
                v-for="child in group.children"
                v-show="!collapsedGroups[group.id]"
                :key="child.id"
                :class="{
                  'docs-sidebar-sub-link': true,
                  'docs-sidebar-active-link': isSectionActive(child.id, 'sub')
                }"
                :to="routeForSection(child.id)"
              >
                {{ child.label }}
              </RouterLink>
            </template>
          </nav>
        </aside>

        <div class="docs-content">
          <BeforeStartSection v-if="currentSection === 'before-start'" />
          <OpenAiSection v-else-if="currentSection === 'openai'" :sub="sub" />
          <AnthropicSection v-else-if="currentSection === 'anthropic'" :sub="sub" />
          <ErrorsSection v-else-if="currentSection === 'errors'" />
          <BillingSection v-else-if="currentSection === 'billing'" />
        </div>
      </div>
    </main>
  </div>
</template>
