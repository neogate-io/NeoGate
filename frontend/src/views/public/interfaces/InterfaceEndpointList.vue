<script setup lang="ts">
import CodeSampleCard from './CodeSampleCard.vue'
import { endpointAnchorFor } from './endpointAnchor'

type ParamRow = string[]

defineProps<{
  items: Array<{
    title: string
    method: string
    path: string
    description?: string
    requestParams?: ParamRow[]
    responseFields?: ParamRow[]
    samples?: Array<{ title: string; code: string }>
  }>
  requestTitle: string
  responseTitle: string
  fieldHeaders: string[]
}>()

// Color-codes the method badge by the leading HTTP verb (GET/POST/DELETE/…).
function methodClass(method: string) {
  return `interface-method--${method.split(/[\s/]/)[0].toLowerCase()}`
}
</script>

<template>
  <div class="interface-endpoint-list">
    <article
      v-for="item in items"
      :id="endpointAnchorFor(item)"
      :key="`${item.method}-${item.path}-${item.title}`"
    >
      <div class="interface-endpoint-heading">
        <h4>{{ item.title }}</h4>
        <div class="interface-endpoint-url">
          <span class="interface-method" :class="methodClass(item.method)">{{ item.method }}</span>
          <code>{{ item.path }}</code>
        </div>
        <p v-if="item.description">{{ item.description }}</p>
      </div>

      <div v-if="item.requestParams?.length" class="docs-params-table-wrap">
        <h5>{{ requestTitle }}</h5>
        <table class="docs-params-table">
          <thead>
            <tr>
              <th v-for="header in fieldHeaders" :key="header">{{ header }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="[name, type, description] in item.requestParams" :key="name">
              <td>
                <code>{{ name }}</code>
              </td>
              <td>{{ type }}</td>
              <td>{{ description }}</td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-if="item.responseFields?.length" class="docs-params-table-wrap">
        <h5>{{ responseTitle }}</h5>
        <table class="docs-params-table">
          <thead>
            <tr>
              <th v-for="header in fieldHeaders" :key="header">{{ header }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="[name, type, description] in item.responseFields" :key="name">
              <td>
                <code>{{ name }}</code>
              </td>
              <td>{{ type }}</td>
              <td>{{ description }}</td>
            </tr>
          </tbody>
        </table>
      </div>

      <CodeSampleCard
        v-for="sample in item.samples"
        :key="sample.title"
        :title="sample.title"
        :code="sample.code"
      />
    </article>
  </div>
</template>
