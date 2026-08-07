<script setup lang="ts">
import { DocumentCopy } from '@element-plus/icons-vue'
import { useLocale } from '../../../composables/useLocale'
import { useCopyText } from '../../../composables/usePublicPage'
import InterfaceEndpointList from './InterfaceEndpointList.vue'
import EndpointOverviewTable from './EndpointOverviewTable.vue'
import CodeSampleCard from './CodeSampleCard.vue'
import { useOpenAiContent } from './openAiContent'

defineProps<{
  sub?: string
}>()

const { t } = useLocale()
const copyDocText = useCopyText()
const { content, quickStart, pythonInstall, python, nodeInstall, node, openAiEndpoints } =
  useOpenAiContent()
</script>

<template>
  <section id="openai" class="docs-section">
    <template v-if="!sub">
      <div class="docs-section-heading">
        <h2>{{ content.openAiTitle }}</h2>
        <p>{{ content.openAiIntro }}</p>
      </div>
      <div class="interface-meta-grid">
        <article v-for="[label, value] in content.openAiAuthItems" :key="label">
          <span>{{ label }}</span>
          <code>{{ value }}</code>
        </article>
      </div>
      <EndpointOverviewTable
        :headers="content.endpointHeaders"
        :rows="openAiEndpoints"
        link-prefix="/interfaces/openai"
        :search-placeholder="content.endpointSearchPlaceholder"
      />
    </template>

    <section v-if="sub === 'quick-start'" id="openai-quick-start" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiQuickStartTitle }}</h2>
      </div>
      <CodeSampleCard title="curl" :code="quickStart" default-open />
    </section>

    <section v-if="sub === 'text'" id="openai-text" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiTextTitle }}</h2>
        <p>{{ content.openAiText }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiTextInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'text-async'" id="openai-text-async" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiTextAsyncTitle }}</h2>
        <p>{{ content.openAiTextAsync }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiTextAsyncInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'images'" id="openai-images" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiImageTitle }}</h2>
        <p>{{ content.openAiImage }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiImageInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'images-async'" id="openai-images-async" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiImageAsyncTitle }}</h2>
        <p>{{ content.openAiImageAsync }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiImageAsyncInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'videos'" id="openai-videos" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiVideoTitle }}</h2>
        <p>{{ content.openAiVideo }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiVideoInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
      <InterfaceEndpointList
        :items="content.openAiAssetInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
      <article class="docs-step-card">
        <h4>{{ content.videoWorkflowTitle }}</h4>
        <div class="docs-check-list docs-inner-check-list">
          <article
            v-for="[title, text] in content.videoWorkflowItems"
            :key="title"
            class="docs-check-item"
          >
            <h3>{{ title }}</h3>
            <p>{{ text }}</p>
          </article>
        </div>
      </article>
      <div class="docs-check-list">
        <article v-for="[title, text] in content.videoNotes" :key="title" class="docs-check-item">
          <h3>{{ title }}</h3>
          <p>{{ text }}</p>
        </article>
      </div>
    </section>

    <section v-if="sub === 'audio'" id="openai-audio" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiAudioTitle }}</h2>
        <p>{{ content.openAiAudio }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiAudioInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'embeddings'" id="openai-embeddings" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiEmbeddingsTitle }}</h2>
        <p>{{ content.openAiEmbeddings }}</p>
      </div>
      <InterfaceEndpointList
        :items="content.openAiEmbeddingsInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'models'" id="openai-models" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiModelsTitle }}</h2>
      </div>
      <InterfaceEndpointList
        :items="content.openAiModelsInterfaces"
        :field-headers="content.paramFieldHeaders"
        :request-title="content.requestParamsTitle"
        :response-title="content.responseParamsTitle"
      />
    </section>

    <section v-if="sub === 'sdk'" id="openai-sdk" class="docs-subsection">
      <div class="docs-section-heading docs-subsection-heading">
        <h2>{{ content.openAiSdkTitle }}</h2>
      </div>
      <article class="docs-step-card">
        <el-tabs>
          <el-tab-pane label="Python">
            <div class="docs-sdk-tab-panel">
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(pythonInstall)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ pythonInstall }}</code></pre>
              </div>
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(python)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ python }}</code></pre>
              </div>
            </div>
          </el-tab-pane>
          <el-tab-pane label="Node.js">
            <div class="docs-sdk-tab-panel">
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(nodeInstall)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ nodeInstall }}</code></pre>
              </div>
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(node)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ node }}</code></pre>
              </div>
            </div>
          </el-tab-pane>
        </el-tabs>
      </article>
    </section>
  </section>
</template>
