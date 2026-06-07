<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { Refresh, Search } from '@element-plus/icons-vue'
import { getUsage } from '../../api/monitoring'
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
const { data: usage, loading, reload } = useAsyncData(() => getUsage(200), [])

const filters = reactive({
  dateRange: [] as string[] | null,
  model: '',
  status: 'all'
})
const currentPage = ref(1)
const pageSize = ref(20)

const filteredUsage = computed(() => {
  const keyword = filters.model.trim().toLowerCase()
  const [start, end] = filters.dateRange ?? []
  const startTime = start ? new Date(`${start}T00:00:00`).getTime() : null
  const endTime = end ? new Date(`${end}T23:59:59.999`).getTime() : null

  return usage.value.filter((row) => {
    const createdAt = new Date(row.created_at).getTime()
    const matchesDate =
      (startTime == null || createdAt >= startTime) && (endTime == null || createdAt <= endTime)
    const matchesModel =
      !keyword ||
      row.provider.toLowerCase().includes(keyword) ||
      (row.model ?? '').toLowerCase().includes(keyword)
    const statusCode = row.status_code ?? 0
    const matchesStatus =
      filters.status === 'all' ||
      (filters.status === 'success' && statusCode >= 200 && statusCode < 400) ||
      (filters.status === 'failed' && (statusCode >= 400 || Boolean(row.error_summary)))
    return matchesDate && matchesModel && matchesStatus
  })
})

const pagedUsage = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return filteredUsage.value.slice(start, start + pageSize.value)
})

watch(
  () => [(filters.dateRange ?? []).join(','), filters.model, filters.status, pageSize.value],
  () => {
    currentPage.value = 1
  }
)

function resetFilters() {
  filters.dateRange = []
  filters.model = ''
  filters.status = 'all'
  currentPage.value = 1
}
</script>

<template>
  <section class="grid usage-view">
    <el-form class="admin-filter-bar user-filter-bar usage-filter-bar" @submit.prevent>
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
        <el-button :icon="Refresh" :loading="loading" @click="reload">{{ t('refresh') }}</el-button>
        <el-button @click="resetFilters">{{ t('reset') }}</el-button>
      </el-form-item>
    </el-form>

    <div class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table usage-table"
        :data="pagedUsage"
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
      <span class="admin-result-count">
        {{ t('filteredResults') }} {{ filteredUsage.length.toLocaleString(locale) }} /
        {{ usage.length.toLocaleString(locale) }}
      </span>
      <el-pagination
        v-model:current-page="currentPage"
        v-model:page-size="pageSize"
        :page-sizes="[20, 50, 100]"
        :total="filteredUsage.length"
        background
        layout="sizes, prev, pager, next"
      />
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
  font-weight: 600;
}

@media (max-width: 900px) {
  .usage-filter-actions.el-form-item {
    margin-left: 0;
  }

  .usage-filter-bar :deep(.el-date-editor),
  .usage-filter-bar :deep(.el-input),
  .usage-filter-bar :deep(.el-select) {
    width: 100%;
  }
}
</style>
