<script setup lang="ts">
import { computed, ref } from 'vue'
import type { EndpointDisplayRow } from './endpointRows'

const props = defineProps<{
  headers: string[]
  rows: EndpointDisplayRow[]
  linkPrefix: string
  searchPlaceholder: string
}>()

const query = ref('')

// Color-codes the method badge by the leading HTTP verb (GET/POST/DELETE/…).
// Combined entries like 'GET/PATCH' or 'GET (WebSocket)' use the first verb.
function methodClass(method: string) {
  return `interface-method--${method.split(/[\s/]/)[0].toLowerCase()}`
}

const filteredRows = computed(() => {
  const keyword = query.value.trim().toLowerCase()
  if (!keyword) return props.rows
  return props.rows.filter((row) =>
    [row.name, row.method, row.path, row.description].some((field) =>
      field.toLowerCase().includes(keyword)
    )
  )
})
</script>

<template>
  <input
    v-model="query"
    type="search"
    class="interface-filter"
    :placeholder="searchPlaceholder"
    :aria-label="searchPlaceholder"
  />
  <div class="interface-table-wrap">
    <table class="interface-table">
      <thead>
        <tr>
          <th v-for="header in headers" :key="header">{{ header }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in filteredRows" :key="`${row.name}-${row.method}-${row.path}`">
          <td>
            <RouterLink
              v-if="row.anchor"
              class="interface-endpoint-link"
              :to="`${linkPrefix}/${row.anchor}`"
            >
              {{ row.name }}
            </RouterLink>
            <template v-else>{{ row.name }}</template>
          </td>
          <td>
            <span class="interface-method" :class="methodClass(row.method)">{{ row.method }}</span>
          </td>
          <td>
            <code>{{ row.path }}</code>
          </td>
          <td>{{ row.description }}</td>
          <td>
            <span
              class="interface-status"
              :class="row.supported ? 'interface-status--ok' : 'interface-status--muted'"
            >
              {{ row.status }}
            </span>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
