<script setup lang="ts">
import { DocumentCopy } from '@element-plus/icons-vue'
import { useLocale } from '../../../composables/useLocale'
import { useCopyText } from '../../../composables/usePublicPage'

withDefaults(
  defineProps<{
    title?: string
    code: string
    collapsible?: boolean
    defaultOpen?: boolean
  }>(),
  {
    title: '',
    collapsible: true,
    defaultOpen: false
  }
)

const { t } = useLocale()
const copyDocText = useCopyText()
</script>

<template>
  <details v-if="collapsible" class="docs-code-details" :open="defaultOpen">
    <summary>
      <h4>{{ title }}</h4>
    </summary>
    <div class="docs-copy-block">
      <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(code)" />
      <pre class="docs-code-sample docs-inner-code"><code>{{ code }}</code></pre>
    </div>
  </details>
  <article v-else class="docs-step-card">
    <h4 v-if="title">{{ title }}</h4>
    <div class="docs-copy-block">
      <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(code)" />
      <pre class="docs-code-sample docs-inner-code"><code>{{ code }}</code></pre>
    </div>
  </article>
</template>
