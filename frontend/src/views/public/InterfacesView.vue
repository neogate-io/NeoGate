<script setup lang="ts">
import { computed, defineAsyncComponent, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import PublicHeader from '../../components/common/PublicHeader.vue'
import { interfacesMenu } from './interfaces/interfacesSections'
import { useLocale } from '../../composables/useLocale'
import { useSiteBrand } from '../../composables/useSiteBrand'

const BeforeStartSection = defineAsyncComponent(() => import('./interfaces/BeforeStartSection.vue'))
const OpenAiSection = defineAsyncComponent(() => import('./interfaces/OpenAiSection.vue'))
const AnthropicSection = defineAsyncComponent(() => import('./interfaces/AnthropicSection.vue'))
const ErrorsSection = defineAsyncComponent(() => import('./interfaces/ErrorsSection.vue'))
const BillingSection = defineAsyncComponent(() => import('./interfaces/BillingSection.vue'))

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
      menuTitle: '目录'
    }
  }

  return {
    title: 'API Reference',
    subtitle: `${siteName.value} follows the official OpenAI / Anthropic API structure. Implemented APIs are callable now; other official APIs show their current support status in the table.`,
    menuTitle: 'Contents'
  }
})

const menuGroups = computed(() => {
  const isZh = locale.value === 'zh-CN'
  return interfacesMenu.map((entry) => ({
    id: entry.id,
    path: entry.path,
    label: isZh ? entry.label.zh : entry.label.en,
    children: (entry.children ?? []).map((child) => ({
      id: child.id,
      path: child.path,
      label: isZh ? child.label.zh : child.label.en
    }))
  }))
})

// Sidebar groups with sub entries (OpenAI / Anthropic) are collapsible. The
// group containing the current route starts expanded, the other collapsed;
// explicit user choices are persisted in localStorage.
const collapsedStorageKey = 'interfaces-sidebar-collapsed'

function readCollapsedGroups(): Record<string, boolean> {
  if (typeof localStorage === 'undefined') return {}
  try {
    const raw = localStorage.getItem(collapsedStorageKey)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, boolean>
    }
  } catch {
    // Ignore malformed stored state and fall back to defaults.
  }
  return {}
}

const collapsedGroups = ref<Record<string, boolean>>({
  openai: !route.path.startsWith('/interfaces/openai'),
  anthropic: !route.path.startsWith('/interfaces/anthropic'),
  ...readCollapsedGroups()
})

watch(
  collapsedGroups,
  (value) => {
    if (typeof localStorage === 'undefined') return
    try {
      localStorage.setItem(collapsedStorageKey, JSON.stringify(value))
    } catch {
      // Storage may be unavailable (e.g. private mode); collapsing still works.
    }
  },
  { deep: true }
)

function toggleGroup(id: string) {
  collapsedGroups.value[id] = !collapsedGroups.value[id]
}

function toggleGroupLabel(group: { id: string; label: string }) {
  const collapsed = collapsedGroups.value[group.id]
  if (locale.value === 'zh-CN') return collapsed ? `展开 ${group.label}` : `折叠 ${group.label}`
  return collapsed ? `Expand ${group.label}` : `Collapse ${group.label}`
}

// Clicking the group title while already on its page toggles the sub entries
// instead of navigating again.
function onGroupClick(id: string, path: string, event: MouseEvent) {
  if (route.path === path) {
    event.preventDefault()
    toggleGroup(id)
  }
}

watch(
  () => route.path,
  (path) => {
    if (path.startsWith('/interfaces/openai')) collapsedGroups.value.openai = false
    if (path.startsWith('/interfaces/anthropic')) collapsedGroups.value.anthropic = false
  },
  // immediate: a stored collapsed state must not hide the group containing the
  // current page when landing on it directly.
  { immediate: true }
)

function isSectionActive(path: string, level?: string) {
  if (level === 'sub') return route.path === path
  // Group headers stay highlighted while on one of their sub pages.
  return route.path === path || route.path.startsWith(`${path}/`)
}

// Flat ordered page list (group pages followed by their sub pages) drives the
// previous/next pager at the bottom of the content column.
const flatPages = computed(() => {
  const isZh = locale.value === 'zh-CN'
  return interfacesMenu.flatMap((entry) => [
    { path: entry.path, label: isZh ? entry.label.zh : entry.label.en },
    ...(entry.children ?? []).map((child) => ({
      path: child.path,
      label: isZh ? child.label.zh : child.label.en
    }))
  ])
})

const pager = computed(() => {
  const index = flatPages.value.findIndex((page) => page.path === route.path)
  return {
    prev: index > 0 ? flatPages.value[index - 1] : null,
    next: index >= 0 && index < flatPages.value.length - 1 ? flatPages.value[index + 1] : null
  }
})

const pagerLabels = computed(() =>
  locale.value === 'zh-CN' ? { prev: '上一页', next: '下一页' } : { prev: 'Previous', next: 'Next' }
)
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
                  :class="{ 'docs-sidebar-active-link': isSectionActive(group.path) }"
                  :to="group.path"
                  @click="onGroupClick(group.id, group.path, $event)"
                >
                  {{ group.label }}
                </RouterLink>
                <button
                  v-if="group.children.length"
                  type="button"
                  class="docs-sidebar-group-toggle"
                  :aria-expanded="!collapsedGroups[group.id]"
                  :aria-label="toggleGroupLabel(group)"
                  @click="toggleGroup(group.id)"
                />
              </div>
              <RouterLink
                v-for="child in group.children"
                v-show="!collapsedGroups[group.id]"
                :key="child.id"
                :class="{
                  'docs-sidebar-sub-link': true,
                  'docs-sidebar-active-link': isSectionActive(child.path, 'sub')
                }"
                :to="child.path"
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

          <nav v-if="pager.prev || pager.next" class="docs-pager" aria-label="Pagination">
            <RouterLink v-if="pager.prev" class="docs-pager-link" :to="pager.prev.path">
              <span>← {{ pagerLabels.prev }}</span>
              <strong>{{ pager.prev.label }}</strong>
            </RouterLink>
            <RouterLink v-if="pager.next" class="docs-pager-link docs-pager-next" :to="pager.next.path">
              <span>{{ pagerLabels.next }} →</span>
              <strong>{{ pager.next.label }}</strong>
            </RouterLink>
          </nav>
        </div>
      </div>
    </main>
  </div>
</template>
