<script setup lang="ts">
import { useLocale } from '../../../composables/useLocale'
import InterfaceEndpointList from './InterfaceEndpointList.vue'
import CodeSampleCard from './CodeSampleCard.vue'
import { useAnthropicContent } from './anthropicContent'

defineProps<{
  sub?: string
}>()

const { locale } = useLocale()
const { content, quickStart, isSupportedStatus, endpointDescription } = useAnthropicContent()
</script>

<template>
  <section id="anthropic" class="docs-section">
    <template v-if="!sub">
      <div class="docs-section-heading">
        <h2>{{ content.anthropicTitle }}</h2>
        <p>{{ content.anthropicIntro }}</p>
      </div>
      <div class="interface-meta-grid">
        <article v-for="[label, value] in content.anthropicAuthItems" :key="label">
          <span>{{ label }}</span>
          <code>{{ value }}</code>
        </article>
      </div>
      <div class="interface-table-wrap">
        <table class="interface-table">
          <thead>
            <tr>
              <th v-for="header in content.endpointHeaders" :key="header">{{ header }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="[name, method, path, , status, anchor] in content.anthropicEndpoints"
              :key="`${name}-${method}-${path}`"
            >
              <td>
                <RouterLink
                  v-if="anchor"
                  class="interface-endpoint-link"
                  :to="`/interfaces/anthropic/${anchor}`"
                >
                  {{ name }}
                </RouterLink>
                <template v-else>{{ name }}</template>
              </td>
              <td>
                <span class="interface-method">{{ method }}</span>
              </td>
              <td>
                <code>{{ path }}</code>
              </td>
              <td>{{ endpointDescription(name, method, path) }}</td>
              <td>
                <span
                  class="interface-status"
                  :class="{ 'interface-status--muted': !isSupportedStatus(status) }"
                >
                  {{ status }}
                </span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>

    <section v-if="sub === 'quick-start'" id="anthropic-quick-start" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.anthropicQuickStartTitle }}</h2>
      </div>
      <CodeSampleCard title="curl" :code="quickStart" default-open />
    </section>

    <section v-if="sub === 'text'" id="anthropic-text" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.anthropicTextTitle }}</h2>
        <p>{{ content.anthropicText }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.anthropicMessageInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="locale.startsWith('zh') ? '请求参数' : 'Request parameters'"
        :response-title="locale.startsWith('zh') ? '响应字段' : 'Response fields'"
      />
    </section>

    <section v-if="sub === 'stream'" id="anthropic-stream" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.anthropicStreamTitle }}</h2>
        <p>{{ content.streamText }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.anthropicStreamInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="locale.startsWith('zh') ? '请求参数' : 'Request parameters'"
        :response-title="locale.startsWith('zh') ? '响应字段' : 'Response fields'"
      />
    </section>

    <section v-if="sub === 'batches'" id="anthropic-batches" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.anthropicBatchesTitle }}</h2>
        <p>{{ content.batchText }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.anthropicBatchInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="locale.startsWith('zh') ? '请求参数' : 'Request parameters'"
        :response-title="locale.startsWith('zh') ? '响应字段' : 'Response fields'"
      />
      <div class="docs-check-list">
        <article v-for="[title, text] in content.batchItems" :key="title" class="docs-check-item">
          <h3>{{ title }}</h3>
          <p>{{ text }}</p>
        </article>
      </div>
    </section>

    <section v-if="sub === 'models'" id="anthropic-models" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.anthropicModelsTitle }}</h2>
        <p>{{ content.anthropicModelsText }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.anthropicModelsInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="locale.startsWith('zh') ? '请求参数' : 'Request parameters'"
        :response-title="locale.startsWith('zh') ? '响应字段' : 'Response fields'"
      />
      <div class="docs-check-list">
        <article
          v-for="[title, text] in content.anthropicModelItems"
          :key="title"
          class="docs-check-item"
        >
          <h3>{{ title }}</h3>
          <p>{{ text }}</p>
        </article>
      </div>
    </section>
  </section>
</template>
