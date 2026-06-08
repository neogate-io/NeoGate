<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { Refresh, Search } from '@element-plus/icons-vue'
import { getAdminUsage, type AdminUsageStatus } from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import {
  cacheWriteTokens,
  formatDateTime,
  formatDurationMs,
  formatMicroUsd,
  formatNumber,
  formatTokenRate
} from '../../utils/format'

const { locale, t } = useLocale()

const filters = reactive({
  dateRange: [] as string[] | null,
  model: '',
  status: 'all' as AdminUsageStatus
})
const currentPage = ref(1)
const pageSize = ref(20)
const cursorStack = ref<(string | undefined)[]>([undefined])

const usageQueryRange = computed(() => {
  const [start, end] = filters.dateRange ?? []
  return {
    start: start ? new Date(`${start}T00:00:00`).toISOString() : undefined,
    end: end ? new Date(`${end}T23:59:59.999`).toISOString() : undefined
  }
})

const {
  data: usagePage,
  loading,
  reload
} = useAsyncData(
  () =>
    getAdminUsage({
      page: currentPage.value,
      limit: pageSize.value,
      start: usageQueryRange.value.start,
      end: usageQueryRange.value.end,
      model: filters.model.trim() || undefined,
      status: filters.status,
      cursor: cursorStack.value[currentPage.value - 1]
    }),
  { items: [], total: 0, page: 1, limit: 20 }
)

const usageItems = computed(() => usagePage.value.items)

async function handleSearch() {
  currentPage.value = 1
  cursorStack.value = [undefined]
  await reload()
}

async function nextPage() {
  if (!usagePage.value.has_more || !usagePage.value.next_cursor) return
  cursorStack.value[currentPage.value] = usagePage.value.next_cursor
  currentPage.value += 1
  await reload()
}

async function previousPage() {
  if (currentPage.value <= 1) return
  currentPage.value -= 1
  await reload()
}

async function handlePageSizeChange(size: number) {
  pageSize.value = size
  currentPage.value = 1
  cursorStack.value = [undefined]
  await reload()
}

async function resetFilters() {
  filters.dateRange = []
  filters.model = ''
  filters.status = 'all'
  currentPage.value = 1
  cursorStack.value = [undefined]
  await reload()
}
</script>

<template>
  <section class="grid usage-view">
    <el-form
      class="admin-filter-bar user-filter-bar usage-filter-bar"
      @submit.prevent="handleSearch"
    >
      <el-form-item :label="t('timeRange')">
        <el-date-picker
          v-model="filters.dateRange"
          type="daterange"
          value-format="YYYY-MM-DD"
          :range-separator="t('to')"
          :start-placeholder="t('startTime')"
          :end-placeholder="t('endTime')"
        />
      </el-form-item>
      <el-form-item :label="t('model')">
        <el-input
          v-model="filters.model"
          clearable
          :prefix-icon="Search"
          :placeholder="t('usageModelSearchPlaceholder')"
        />
      </el-form-item>
      <el-form-item :label="t('status')">
        <el-select v-model="filters.status">
          <el-option :label="t('usageStatusAll')" value="all" />
          <el-option :label="t('usageStatusSuccess')" value="success" />
          <el-option :label="t('usageStatusFailed')" value="failed" />
        </el-select>
      </el-form-item>
      <el-form-item class="user-search-actions usage-filter-actions">
        <el-button type="primary" native-type="submit" :icon="Search" :loading="loading">
          {{ t('search') }}
        </el-button>
        <el-button :icon="Refresh" :loading="loading" @click="reload">{{ t('refresh') }}</el-button>
        <el-button @click="resetFilters">{{ t('reset') }}</el-button>
      </el-form-item>
    </el-form>

    <div class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table usage-table"
        :data="usageItems"
        stripe
      >
        <el-table-column :label="t('time')" min-width="180">
          <template #default="{ row }">{{ formatDateTime(row.created_at, locale) }}</template>
        </el-table-column>
        <el-table-column :label="t('model')" min-width="190">
          <template #default="{ row }">
            <div class="usage-model">
              <span class="usage-provider">{{ row.provider }}</span>
              <span>{{ row.model || '-' }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('latency')" min-width="170">
          <template #default="{ row }">
            <div class="usage-stack">
              <div class="usage-tags">
                <el-tag type="success" effect="plain" round>{{
                  formatDurationMs(row.latency_ms)
                }}</el-tag>
                <el-tag v-if="row.first_response_ms != null" type="success" effect="plain" round>
                  {{ formatDurationMs(row.first_response_ms) }}
                </el-tag>
              </div>
              <span class="usage-muted">
                {{ row.streamed ? t('streamLabel') : t('nonStreamLabel') }}
                <template v-if="formatTokenRate(row.output_tokens_per_second, locale, '')">
                  · {{ formatTokenRate(row.output_tokens_per_second, locale, '') }}</template
                >
              </span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('tokens')" min-width="180">
          <template #default="{ row }">
            <div class="usage-stack">
              <span class="usage-mono">
                {{ formatNumber(row.input_tokens, locale) }} /
                {{ formatNumber(row.output_tokens, locale) }}
              </span>
              <span
                v-if="row.cache_in_tokens || cacheWriteTokens(row) || row.reason_out_tokens"
                class="usage-muted"
              >
                <template v-if="row.cache_in_tokens"
                  >{{ t('cacheRead') }}↓ {{ formatNumber(row.cache_in_tokens, locale) }}</template
                >
                <template v-if="cacheWriteTokens(row)">
                  ↑ {{ formatNumber(cacheWriteTokens(row), locale) }}</template
                >
                <template v-if="row.reason_out_tokens">
                  · {{ t('reasoning') }} {{ formatNumber(row.reason_out_tokens, locale) }}</template
                >
              </span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('cost')">
          <template #default="{ row }">{{ formatMicroUsd(row.cost_micro_usd, 6) }}</template>
        </el-table-column>
        <el-table-column prop="billing_status" :label="t('billing')" />
        <el-table-column :label="t('status')" min-width="100">
          <template #default="{ row }">{{ row.status_code || '-' }}</template>
        </el-table-column>
        <el-table-column prop="error_summary" :label="t('error')" min-width="180">
          <template #default="{ row }">{{ row.error_summary || '-' }}</template>
        </el-table-column>
        <template #empty>
          <el-empty :description="t('noData')" />
        </template>
      </el-table>
    </div>

    <div class="admin-pagination-bar">
      <div class="admin-pagination-summary">
        <span class="admin-result-count">
          {{ t('currentPageItems') }} {{ usageItems.length.toLocaleString(locale) }}
          {{ t('itemsUnit') }}
        </span>
      </div>
      <div class="admin-pagination-controls">
        <div class="admin-page-size-control">
          <span class="admin-page-label">{{ t('pageSize') }}</span>
          <el-select v-model="pageSize" class="admin-page-size" @change="handlePageSizeChange">
            <el-option :value="20" label="20" />
            <el-option :value="50" label="50" />
            <el-option :value="100" label="100" />
          </el-select>
        </div>
        <span class="admin-result-count">{{ t('currentPage') }} {{ currentPage }}</span>
        <div class="admin-page-buttons">
          <el-button :disabled="currentPage <= 1 || loading" @click="previousPage">
            {{ t('previousPage') }}
          </el-button>
          <el-button :disabled="!usagePage.has_more || loading" @click="nextPage">
            {{ t('nextPage') }}
          </el-button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.usage-model,
.usage-stack {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.usage-provider,
.usage-muted {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

.usage-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.usage-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-weight: 400;
}
</style>
