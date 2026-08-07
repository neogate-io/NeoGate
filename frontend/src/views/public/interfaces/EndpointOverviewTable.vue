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
            <span class="interface-method">{{ row.method }}</span>
          </td>
          <td>
            <code>{{ row.path }}</code>
          </td>
          <td>{{ row.description }}</td>
          <td>
            <span class="interface-status" :class="{ 'interface-status--muted': !row.supported }">
              {{ row.status }}
            </span>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
