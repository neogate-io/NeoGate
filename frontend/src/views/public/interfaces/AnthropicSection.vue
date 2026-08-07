<script setup lang="ts">
import InterfaceEndpointList from './InterfaceEndpointList.vue'
import EndpointOverviewTable from './EndpointOverviewTable.vue'
import CodeSampleCard from './CodeSampleCard.vue'
import { useAnthropicContent } from './anthropicContent'

defineProps<{
  sub?: string
}>()

const { content, quickStart, anthropicEndpoints } = useAnthropicContent()
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
      <EndpointOverviewTable
        :headers="content.endpointHeaders"
        :rows="anthropicEndpoints"
        link-prefix="/interfaces/anthropic"
        :search-placeholder="content.endpointSearchPlaceholder"
      />
    </template>

    <section v-if="sub === 'quick-start'" id="anthropic-quick-start" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.anthropicQuickStartTitle }}</h2>
        <p>{{ content.anthropicQuickStartIntro }}</p>
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
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
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
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
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
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
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
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
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
