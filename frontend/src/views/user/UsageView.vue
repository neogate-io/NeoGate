<script setup lang="ts">
import { computed, ref } from 'vue'
import { Download, Refresh } from '@element-plus/icons-vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { getUserUsage } from '../../api/monitoring'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'

const { locale, t } = useLocale()
const currentPage = ref(1)
const pageSize = ref(20)
const dateRange = ref<[Date, Date] | null>(null)
const usageQueryRange = computed(() => {
  if (!dateRange.value) return { start: undefined, end: undefined }
  const [startDate, endDate] = dateRange.value
  return {
    start: startDate.toISOString(),
    end: endDate.toISOString()
  }
})
const { data: usagePage, loading, reload } = useAsyncData(
  () => getUserUsage(currentPage.value, pageSize.value, usageQueryRange.value.start, usageQueryRange.value.end),
  { items: [], total: 0, page: 1, limit: 20 }
)

const filteredItems = computed(() => usagePage.value.items)

function formatUsd(microUsd?: number | null, digits = 6) {
  if (microUsd == null) return '-'
  return `$${(microUsd / 1_000_000).toFixed(digits)}`
}

function formatMs(ms?: number | null) {
  if (ms == null) return '-'
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`
}

function formatNumber(value?: number | null) {
  return value == null ? '-' : value.toLocaleString(locale.value)
}

function formatFullTime(value: string) {
  return new Date(value).toLocaleString(locale.value, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
}

function cacheWriteTokens(row: { cache_create_in_tokens?: number | null; cache_create_5m_in_tokens?: number | null; cache_create_1h_in_tokens?: number | null }) {
  const split = (row.cache_create_5m_in_tokens || 0) + (row.cache_create_1h_in_tokens || 0)
  return split > 0 ? split : row.cache_create_in_tokens || 0
}

function formatRate(value?: number | null) {
  if (value == null || value <= 0) return '-'
  return `${Math.round(value).toLocaleString(locale.value)} t/s`
}

function statusLabel(statusCode?: number | null) {
  return statusCode && statusCode >= 400 ? String(statusCode) : t('success')
}

async function handlePageChange(page: number) {
  currentPage.value = page
  await reload()
}

async function handlePageSizeChange(size: number) {
  pageSize.value = size
  currentPage.value = 1
  await reload()
}

async function handleDateRangeChange() {
  currentPage.value = 1
  await reload()
}

async function exportUsage() {
  const exportPage = await getUserUsage(1, 1000, usageQueryRange.value.start, usageQueryRange.value.end)
  const headers = [
    t('time'),
    t('model'),
    t('provider'),
    t('inputShort'),
    t('outputShort'),
    t('tokens'),
    t('cacheReadExport'),
    t('cacheWriteShort'),
    t('totalLatency'),
    t('firstResponseLatency'),
    t('throughput'),
    t('cost'),
    t('status')
  ]
  const rows = exportPage.items.map((row) => [
    formatFullTime(row.created_at),
    row.model || '',
    row.provider,
    row.input_tokens ?? '',
    row.output_tokens ?? '',
    row.total_tokens ?? '',
    row.cache_in_tokens ?? '',
    cacheWriteTokens(row),
    formatMs(row.latency_ms),
    formatMs(row.first_response_ms),
    formatRate(row.output_tokens_per_second),
    formatUsd(row.cost_micro_usd),
    statusLabel(row.status_code)
  ])
  const csv = [headers, ...rows].map((row) => row.map(escapeCsvCell).join(',')).join('\n')
  const blob = new Blob([`\uFEFF${csv}`], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `usage-${new Date().toISOString().slice(0, 10)}.csv`
  link.click()
  URL.revokeObjectURL(url)
}

function escapeCsvCell(value: string | number) {
  const text = String(value)
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
}
</script>

<template>
  <section class="usage-view">
    <div v-loading="loading" class="user-panel usage-console-panel">
      <div class="usage-toolbar">
        <div class="usage-toolbar-title">
          <h3>{{ t('usageRecords') }}</h3>
        </div>
        <div class="usage-toolbar-actions">
          <el-date-picker
            v-model="dateRange"
            class="usage-date-range"
            clearable
            type="datetimerange"
            :start-placeholder="t('startTime')"
            :end-placeholder="t('endTime')"
            @change="handleDateRangeChange"
          />
          <el-button :icon="Download" :disabled="filteredItems.length === 0" @click="exportUsage">
            {{ t('exportDetails') }}
          </el-button>
          <el-tooltip :content="t('refresh')" placement="top">
            <el-button :icon="Refresh" :loading="loading" @click="reload" />
          </el-tooltip>
        </div>
      </div>

      <div class="usage-list">
        <div class="usage-table-header" role="row">
          <span>{{ t('time') }}</span>
          <span>{{ t('model') }}</span>
          <span>{{ t('tokensColumnHint') }}</span>
          <span>{{ t('latencyColumnHint') }}</span>
          <span>{{ t('cost') }}</span>
          <span>{{ t('actions') }}</span>
        </div>
        <details v-for="row in filteredItems" :key="row.id" class="usage-row">
          <summary>
            <span class="usage-time">{{ formatFullTime(row.created_at) }}</span>
            <span class="usage-model-cell">
              <span class="usage-model-pill">
                <ProviderIcon :provider="row.provider" />
                <span>{{ row.model || '-' }}</span>
              </span>
            </span>
            <span class="usage-token-stack">
              <span>{{ formatNumber(row.input_tokens) }} / {{ formatNumber(row.output_tokens) }}</span>
              <small>{{ t('cacheReadShort') }}↓ {{ formatNumber(row.cache_in_tokens) }}</small>
            </span>
            <span class="usage-latency-cell">
              <span class="usage-latency-pills">
                <b>{{ formatMs(row.latency_ms) }}</b>
                <b>{{ formatMs(row.first_response_ms) }}</b>
              </span>
              <small>{{ row.streamed ? t('streamShortLabel') : t('nonStreamShortLabel') }} · {{ formatRate(row.output_tokens_per_second) }}</small>
            </span>
            <span class="usage-cost">{{ formatUsd(row.cost_micro_usd) }}</span>
            <span class="usage-details-label">{{ t('viewDetails') }}</span>
          </summary>
          <dl class="usage-detail-grid">
            <div>
              <dt>{{ t('provider') }}</dt>
              <dd>{{ row.provider }}</dd>
            </div>
            <div>
              <dt>{{ t('time') }}</dt>
              <dd>{{ formatFullTime(row.created_at) }}</dd>
            </div>
            <div>
              <dt>{{ t('model') }}</dt>
              <dd>{{ row.model || '-' }}</dd>
            </div>
            <div>
              <dt>{{ t('responseMode') }}</dt>
              <dd>{{ row.streamed ? t('streamLabel') : t('nonStreamLabel') }}</dd>
            </div>
            <div>
              <dt>{{ t('totalTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.total_tokens) }}</dd>
            </div>
            <div>
              <dt>{{ t('inputTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.input_tokens) }}</dd>
            </div>
            <div>
              <dt>{{ t('outputTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.output_tokens) }}</dd>
            </div>
            <div>
              <dt>{{ t('cacheReadTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.cache_in_tokens) }}</dd>
            </div>
            <div>
              <dt>{{ t('cacheWriteTokensDetail') }}</dt>
              <dd>{{ formatNumber(cacheWriteTokens(row)) }}</dd>
            </div>
            <div>
              <dt>{{ t('totalLatencyDetail') }}</dt>
              <dd>{{ formatMs(row.latency_ms) }}</dd>
            </div>
            <div>
              <dt>{{ t('firstResponseLatencyDetail') }}</dt>
              <dd>{{ formatMs(row.first_response_ms) }}</dd>
            </div>
            <div>
              <dt>{{ t('status') }}</dt>
              <dd>
                <span class="usage-status" :class="{ 'is-error': row.status_code && row.status_code >= 400 }">
                  {{ statusLabel(row.status_code) }}
                </span>
              </dd>
            </div>
            <div>
              <dt>{{ t('throughput') }}</dt>
              <dd>{{ formatRate(row.output_tokens_per_second) }}</dd>
            </div>
            <div>
              <dt>{{ t('cost') }}</dt>
              <dd>{{ formatUsd(row.cost_micro_usd) }}</dd>
            </div>
            <div>
              <dt>{{ t('billingStatus') }}</dt>
              <dd>{{ row.billing_status || '-' }}</dd>
            </div>
            <div>
              <dt>{{ t('errorSummary') }}</dt>
              <dd>{{ row.error_summary || '-' }}</dd>
            </div>
          </dl>
        </details>
        <div v-if="loading && filteredItems.length === 0" class="usage-loading-rows" aria-hidden="true">
          <span v-for="index in pageSize" :key="index"></span>
        </div>
      </div>

      <div class="usage-pagination">
        <el-pagination
          v-model:current-page="currentPage"
          v-model:page-size="pageSize"
          background
          layout="total, sizes, prev, pager, next"
          :page-sizes="[10, 20, 50, 100]"
          :total="usagePage.total"
          @current-change="handlePageChange"
          @size-change="handlePageSizeChange"
        />
      </div>
    </div>
  </section>
</template>

<style scoped>
.usage-view {
  display: grid;
  gap: 12px;
  width: min(1120px, 100%);
}

.usage-console-panel {
  border-color: #dfe5ee;
  box-shadow: 0 18px 44px rgba(15, 23, 42, 0.035);
  overflow: hidden;
}

.usage-toolbar {
  align-items: center;
  border-bottom: 1px solid #edf1f6;
  display: flex;
  gap: 16px;
  justify-content: space-between;
  min-height: 72px;
  padding: 18px 20px;
}

.usage-toolbar-title {
  display: grid;
  gap: 5px;
  min-width: 0;
}

.usage-toolbar-title h3 {
  color: #111827;
  font-size: 18px;
  font-weight: 650;
  line-height: 1.2;
  margin: 0;
}

.usage-toolbar-actions {
  align-items: center;
  display: flex;
  gap: 10px;
}

.usage-date-range {
  width: min(260px, 28vw);
}

.usage-toolbar-actions :deep(.usage-date-range.el-date-editor--datetimerange.el-input__wrapper) {
  flex: 0 0 auto;
  width: 280px !important;
}

.usage-toolbar-actions :deep(.el-button) {
  border-radius: 8px;
  font-size: 14px;
  font-weight: 540;
  height: 36px;
  padding: 0 14px;
}

.usage-toolbar-actions :deep(.el-date-editor) {
  --el-input-border-radius: 8px;
  height: 36px;
}

.usage-toolbar-actions :deep(.el-range-input) {
  color: #4b5565;
  font-size: 14px;
  font-weight: 500;
}

.usage-toolbar-actions :deep(.el-range-separator) {
  color: #697586;
  font-size: 14px;
  font-weight: 500;
}

.usage-list {
  display: grid;
}

.usage-table-header {
  align-items: center;
  background: #f8fafc;
  border-bottom: 1px solid #e5eaf1;
  color: #697586;
  display: grid;
  font-size: 12px;
  font-weight: 500;
  gap: 12px;
  grid-template-columns: 164px minmax(150px, 210px) 168px 156px 104px 82px;
  min-height: 48px;
  padding: 0 22px;
}

.usage-table-header span {
  min-width: 0;
}

.usage-loading-rows {
  display: grid;
}

.usage-loading-rows span {
  border-bottom: 1px solid #edf1f6;
  min-height: 78px;
}

.usage-row {
  border-bottom: 1px solid #edf1f6;
  outline: none;
}

.usage-row summary {
  align-items: center;
  cursor: pointer;
  display: grid;
  gap: 12px;
  grid-template-columns: 164px minmax(150px, 210px) 168px 156px 104px 82px;
  list-style: none;
  min-height: 78px;
  outline: none;
  padding: 0 22px;
}

.usage-row summary:focus-visible {
  background: #f8fbff;
  box-shadow: inset 3px 0 0 var(--user-primary, #168bd3);
}

.usage-row summary::-webkit-details-marker {
  display: none;
}

.usage-row summary:hover {
  background: #fbfdff;
}

.usage-time,
.usage-model-cell,
.usage-token-stack,
.usage-latency-cell,
.usage-cost,
.usage-status {
  color: #111827;
  font-size: 12.5px;
  font-weight: 400;
}

.usage-time {
  color: #475467;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  font-size: 12px;
  font-weight: 400;
  white-space: nowrap;
}

.usage-model-cell,
.usage-token-stack,
.usage-latency-cell {
  display: grid;
  gap: 6px;
  line-height: 1.15;
  min-width: 0;
}

.usage-model-cell small,
.usage-token-stack small,
.usage-latency-cell small {
  color: #9a9a9a;
  font-size: 12px;
  font-weight: 400;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-model-pill {
  align-items: center;
  background: #ffffff;
  border: 1px solid #e1e6ee;
  border-radius: 8px;
  box-shadow: 0 1px 1px rgba(15, 23, 42, 0.02);
  display: inline-flex;
  gap: 8px;
  justify-self: start;
  max-width: 100%;
  min-height: 36px;
  padding: 0 12px;
}

.usage-model-pill > span {
  color: #111827;
  font-size: 14px;
  font-weight: 400;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-token-stack > span {
  color: #111827;
  font-size: 14px;
  font-weight: 400;
  white-space: nowrap;
}

.usage-token-stack span,
.usage-latency-cell span,
.usage-latency-cell small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-token-stack span + span {
  color: #697586;
  font-size: 12px;
  font-weight: 400;
}

.usage-latency-pills {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.usage-latency-pills b {
  align-items: center;
  background: #edfcf7;
  border: 1px solid #b6f0dc;
  border-radius: 7px;
  color: #0f8f70;
  display: inline-flex;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  font-size: 12px;
  font-weight: 400;
  min-height: 24px;
  padding: 0 7px;
}

.usage-latency-pills b:first-child::before {
  background: #20c997;
  border-radius: 999px;
  content: "";
  height: 6px;
  margin-right: 6px;
  width: 6px;
}

.usage-cost {
  color: #111827;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  font-size: 12.5px;
  font-weight: 400;
  text-align: left;
}

.usage-status {
  background: #ecfdf3;
  border: 1px solid #bbf7d0;
  border-radius: 999px;
  color: #047857;
  font-size: 12px;
  font-weight: 400;
  justify-self: start;
  padding: 4px 10px;
}

.usage-status.is-error {
  background: #fef2f2;
  border-color: #fecaca;
  color: #b42318;
}

.usage-details-label {
  color: var(--user-primary, #168bd3);
  font-size: 12px;
  font-weight: 400;
  text-align: left;
  white-space: nowrap;
}

.usage-detail-grid {
  background: #ffffff;
  border-top: 1px solid #f4f7fb;
  display: grid;
  gap: 14px 18px;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  margin: 0;
  padding: 18px 22px 20px;
}

.usage-detail-grid div {
  display: grid;
  gap: 5px;
}

.usage-detail-grid dt {
  color: #8a95a5;
  font-size: 12px;
  font-weight: 400;
}

.usage-detail-grid dd {
  color: #111827;
  font-size: 12.5px;
  font-weight: 400;
  margin: 0;
  overflow-wrap: anywhere;
}

.usage-pagination {
  align-items: center;
  background: #ffffff;
  border-top: 1px solid #edf1f6;
  display: flex;
  justify-content: flex-end;
  min-height: 62px;
  padding: 12px 20px;
}

@media (max-width: 820px) {
  .usage-table-header {
    display: none;
  }

  .usage-detail-grid {
    grid-template-columns: 1fr;
  }

  .usage-toolbar {
    align-items: stretch;
    display: grid;
    padding: 16px;
  }

  .usage-toolbar-actions {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
  }

  .usage-date-range {
    width: 100%;
  }

  .usage-row summary {
    gap: 8px 12px;
    grid-template-areas:
      "model model"
      "time cost"
      "tokens latency"
      "details details";
    grid-template-columns: minmax(0, 1fr) auto;
    min-height: 0;
    padding: 14px 16px;
  }

  .usage-time {
    color: #697586;
    grid-area: time;
    font-size: 12px;
    font-weight: 400;
    white-space: normal;
  }

  .usage-model-cell {
    grid-area: model;
  }

  .usage-token-stack {
    grid-area: tokens;
  }

  .usage-latency-cell {
    grid-area: latency;
    justify-self: end;
    text-align: right;
  }

  .usage-cost {
    grid-area: cost;
    text-align: right;
  }

  .usage-details-label {
    grid-area: details;
    justify-self: start;
    text-align: left;
  }
}
</style>
