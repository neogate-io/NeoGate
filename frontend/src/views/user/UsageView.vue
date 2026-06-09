<script setup lang="ts">
import { computed, ref } from 'vue'
import { Download, Refresh } from '@element-plus/icons-vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { getUserUsage } from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import {
  cacheWriteTokens,
  downloadCsv,
  formatDateTime,
  formatDurationMs,
  formatMicroUsd,
  formatNumber,
  formatTokenRate
} from '../../utils/format'

const { locale, t } = useLocale()
const DEFAULT_PAGE_SIZE = 20
const loadingRowCount = 3
const dateRange = ref<[Date, Date] | null>(null)
const {
  currentPage,
  pageSize,
  currentCursor,
  reset: resetCursorPagination,
  goToNext,
  goToPrevious
} = useCursorPagination(DEFAULT_PAGE_SIZE)
const usageQueryRange = computed(() => {
  if (!dateRange.value) return { start: undefined, end: undefined }
  const [startDate, endDate] = dateRange.value
  return {
    start: startDate.toISOString(),
    end: endDate.toISOString()
  }
})
const {
  data: usagePage,
  loading,
  loaded: usageLoaded,
  reload
} = useAsyncData(
  () =>
    getUserUsage(
      currentPage.value,
      pageSize.value,
      usageQueryRange.value.start,
      usageQueryRange.value.end,
      currentCursor.value
    ),
  { items: [], total: 0, page: 1, limit: DEFAULT_PAGE_SIZE }
)

const filteredItems = computed(() => usagePage.value.items)
const usageInitialLoading = computed(() => !usageLoaded.value)
const hasUsagePagination = computed(
  () => currentPage.value > 1 || Boolean(usagePage.value.has_more)
)

function formatFullTime(value: string) {
  return formatDateTime(value, locale.value, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
}

function statusLabel(statusCode?: number | null) {
  return statusCode && statusCode >= 400 ? String(statusCode) : t('success')
}

async function nextPage() {
  if (!usagePage.value.has_more || !usagePage.value.next_cursor) return
  goToNext(usagePage.value.next_cursor)
  await reload()
}

async function previousPage() {
  if (!goToPrevious()) return
  await reload()
}

async function handlePageSizeChange(size: number) {
  pageSize.value = size
  resetCursorPagination()
  await reload()
}

async function handleDateRangeChange() {
  resetCursorPagination()
  await reload()
}

async function exportUsage() {
  const exportPage = await getUserUsage(
    1,
    1000,
    usageQueryRange.value.start,
    usageQueryRange.value.end
  )
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
    formatDurationMs(row.latency_ms),
    formatDurationMs(row.first_response_ms),
    formatTokenRate(row.output_tokens_per_second, locale.value),
    formatMicroUsd(row.cost_micro_usd, 6),
    statusLabel(row.status_code)
  ])
  downloadCsv(`usage-${new Date().toISOString().slice(0, 10)}.csv`, [headers, ...rows])
}
</script>

<template>
  <section class="usage-view">
    <div v-loading="loading && filteredItems.length > 0" class="user-panel usage-console-panel">
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
        <div v-if="filteredItems.length > 0" class="usage-table-header" role="row">
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
              <span
                >{{ formatNumber(row.input_tokens, locale) }} /
                {{ formatNumber(row.output_tokens, locale) }}</span
              >
              <small
                >{{ t('cacheReadShort') }}↓ {{ formatNumber(row.cache_in_tokens, locale) }}</small
              >
            </span>
            <span class="usage-latency-cell">
              <span class="usage-latency-pills">
                <b>{{ formatDurationMs(row.latency_ms) }}</b>
                <b>{{ formatDurationMs(row.first_response_ms) }}</b>
              </span>
              <small
                >{{ row.streamed ? t('streamShortLabel') : t('nonStreamShortLabel') }} ·
                {{ formatTokenRate(row.output_tokens_per_second, locale) }}</small
              >
            </span>
            <span class="usage-cost">{{ formatMicroUsd(row.cost_micro_usd, 6) }}</span>
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
              <dd>{{ formatNumber(row.total_tokens, locale) }}</dd>
            </div>
            <div>
              <dt>{{ t('inputTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.input_tokens, locale) }}</dd>
            </div>
            <div>
              <dt>{{ t('outputTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.output_tokens, locale) }}</dd>
            </div>
            <div>
              <dt>{{ t('cacheReadTokensDetail') }}</dt>
              <dd>{{ formatNumber(row.cache_in_tokens, locale) }}</dd>
            </div>
            <div>
              <dt>{{ t('cacheWriteTokensDetail') }}</dt>
              <dd>{{ formatNumber(cacheWriteTokens(row), locale) }}</dd>
            </div>
            <div>
              <dt>{{ t('totalLatencyDetail') }}</dt>
              <dd>{{ formatDurationMs(row.latency_ms) }}</dd>
            </div>
            <div>
              <dt>{{ t('firstResponseLatencyDetail') }}</dt>
              <dd>{{ formatDurationMs(row.first_response_ms) }}</dd>
            </div>
            <div>
              <dt>{{ t('status') }}</dt>
              <dd>
                <span
                  class="usage-status"
                  :class="{ 'is-error': row.status_code && row.status_code >= 400 }"
                >
                  {{ statusLabel(row.status_code) }}
                </span>
              </dd>
            </div>
            <div>
              <dt>{{ t('throughput') }}</dt>
              <dd>{{ formatTokenRate(row.output_tokens_per_second, locale) }}</dd>
            </div>
            <div>
              <dt>{{ t('cost') }}</dt>
              <dd>{{ formatMicroUsd(row.cost_micro_usd, 6) }}</dd>
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
        <div
          v-if="loading && filteredItems.length === 0"
          class="usage-loading-rows"
          aria-hidden="true"
        >
          <span v-for="index in loadingRowCount" :key="index">
            <i></i>
            <i></i>
            <i></i>
          </span>
        </div>
        <div v-else-if="filteredItems.length === 0" class="usage-empty-state">
          <el-empty :description="t('noData')" />
        </div>
      </div>
    </div>

    <div
      v-if="!usageInitialLoading && (hasUsagePagination || filteredItems.length > 1)"
      class="admin-pagination-bar"
    >
      <div class="admin-pagination-summary">
        <span class="admin-result-count">
          {{ t('currentPageItems') }} {{ filteredItems.length.toLocaleString(locale) }}
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
  gap: 12px;
  min-height: 220px;
  padding: 24px 22px;
}

.usage-loading-rows span {
  align-items: center;
  background: #fbfdff;
  border: 1px solid #edf1f6;
  border-radius: 8px;
  display: grid;
  gap: 16px;
  grid-template-columns: 156px minmax(160px, 1fr) 120px;
  min-height: 54px;
  padding: 0 16px;
}

.usage-loading-rows i {
  background: linear-gradient(90deg, #eef3f8 0%, #f8fafc 48%, #eef3f8 100%);
  background-size: 220% 100%;
  border-radius: 999px;
  display: block;
  height: 12px;
}

.usage-loading-rows i:nth-child(2) {
  max-width: 280px;
}

.usage-loading-rows i:nth-child(3) {
  max-width: 96px;
}

.usage-empty-state {
  align-items: center;
  display: flex;
  justify-content: center;
  min-height: 260px;
  padding: 32px 20px;
}

.usage-empty-state :deep(.el-empty) {
  --el-empty-fill-color-1: #f8fafc;
  --el-empty-fill-color-2: #eef3f8;
  --el-empty-fill-color-3: #dfe7ef;
  --el-empty-fill-color-4: #cbd7e5;
  --el-empty-fill-color-5: #e6edf4;
  padding: 0;
}

.usage-empty-state :deep(.el-empty__description) {
  margin-top: 12px;
}

.usage-empty-state :deep(.el-empty__description p) {
  color: #697586;
  font-size: 14px;
  font-weight: 540;
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
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
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
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-size: 12px;
  font-weight: 400;
  min-height: 24px;
  padding: 0 7px;
}

.usage-latency-pills b:first-child::before {
  background: #20c997;
  border-radius: 999px;
  content: '';
  height: 6px;
  margin-right: 6px;
  width: 6px;
}

.usage-cost {
  color: #111827;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
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

  .usage-loading-rows {
    min-height: 200px;
    padding: 18px 16px;
  }

  .usage-loading-rows span {
    gap: 10px;
    grid-template-columns: minmax(0, 1fr);
    padding: 14px;
  }

  .usage-loading-rows i:nth-child(2) {
    max-width: 72%;
  }

  .usage-loading-rows i:nth-child(3) {
    max-width: 44%;
  }

  .usage-row summary {
    gap: 8px 12px;
    grid-template-areas:
      'model model'
      'time cost'
      'tokens latency'
      'details details';
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
