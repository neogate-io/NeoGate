<script setup lang="ts">
import { computed, ref } from 'vue'
import { Download, Refresh } from '@element-plus/icons-vue'
import { getUserUsage } from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPageActions } from '../../composables/useCursorPageActions'
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
  goToNext,
  goToPrevious,
  reset: resetCursorPagination
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

const usageInitialLoading = computed(() => !usageLoaded.value)
const hasUsagePagination = computed(
  () => currentPage.value > 1 || Boolean(usagePage.value.has_more)
)
const {
  resetAndReload,
  nextPage,
  previousPage,
  handlePageSizeChange
} = useCursorPageActions(
  { pageSize, reset: resetCursorPagination, goToNext, goToPrevious },
  () => usagePage.value,
  reload
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

function formatClockTime(value: string) {
  return formatDateTime(value, locale.value, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
}

function statusLabel(statusCode?: number | null) {
  return statusCode && statusCode >= 400 ? String(statusCode) : t('success')
}

function routingTierLabel(tier?: string | null) {
  if (tier === 'simple') return t('routingTier_simple')
  if (tier === 'standard') return t('routingTier_standard')
  if (tier === 'advanced') return t('routingTier_advanced')
  return tier || '-'
}

function routingReasonText(row: (typeof usagePage.value.items)[number]) {
  const code = row.routing?.reason_code
  if (code === 'selected_priority_weight') return t('routingReason_selected_priority_weight')
  if (code === 'fallback_no_candidate') return t('routingReason_fallback_no_candidate')
  if (code === 'missing_context') return t('routingReason_missing_context')
  if (code === 'complex_signal') return t('routingReason_complex_signal')
  if (code === 'medium_signal') return t('routingReason_medium_signal')
  if (code === 'simple_signal') return t('routingReason_simple_signal')
  return code || '-'
}

function routingTaskText(taskType?: string | null) {
  if (taskType === 'vision') return t('routingTask_vision')
  if (taskType === 'tool_use') return t('routingTask_tool_use')
  if (taskType === 'structured_output') return t('routingTask_structured_output')
  if (taskType === 'reasoning') return t('routingTask_reasoning')
  if (taskType === 'code') return t('routingTask_code')
  if (taskType === 'translation') return t('routingTask_translation')
  if (taskType === 'summarization') return t('routingTask_summarization')
  if (taskType === 'extraction') return t('routingTask_extraction')
  if (taskType === 'long_context') return t('routingTask_long_context')
  if (taskType === 'chat') return t('routingTask_chat')
  if (taskType === 'unknown') return t('routingTask_unknown')
  return taskType || '-'
}

function routingRuleText(ruleId: string) {
  if (ruleId === 'missing_context') return t('routingRule_missing_context')
  if (ruleId === 'has_images') return t('routingRule_has_images')
  if (ruleId === 'reasoning_effort') return t('routingRule_reasoning_effort')
  if (ruleId === 'reasoning_keywords') return t('routingRule_reasoning_keywords')
  if (ruleId === 'very_long_context') return t('routingRule_very_long_context')
  if (ruleId === 'long_context') return t('routingRule_long_context')
  if (ruleId === 'has_tools') return t('routingRule_has_tools')
  if (ruleId === 'has_response_format') return t('routingRule_has_response_format')
  if (ruleId === 'code_signal') return t('routingRule_code_signal')
  if (ruleId === 'multi_turn_context') return t('routingRule_multi_turn_context')
  if (ruleId === 'translation_signal') return t('routingRule_translation_signal')
  if (ruleId === 'summarization_signal') return t('routingRule_summarization_signal')
  if (ruleId === 'extraction_signal') return t('routingRule_extraction_signal')
  if (ruleId === 'short_plain_text') return t('routingRule_short_plain_text')
  return ruleId
}

function routingRulesText(row: (typeof usagePage.value.items)[number]) {
  return row.routing?.matched_rule_ids.map(routingRuleText).join(locale.value === 'zh-CN' ? '；' : '; ') || ''
}

function routingCandidatesText(row: (typeof usagePage.value.items)[number]) {
  return (
    row.routing?.candidate_summary
      .map((candidate) => `${candidate.target_model} P${candidate.priority}/W${candidate.weight}`)
      .join(locale.value === 'zh-CN' ? '；' : '; ') || ''
  )
}

function closeOtherUsageRows(current: HTMLDetailsElement) {
  current
    .closest('.usage-list')
    ?.querySelectorAll('details[open]')
    .forEach((el) => {
      if (el !== current) (el as HTMLDetailsElement).open = false
    })
}

function toggleUsageDetails(event: MouseEvent) {
  const details = (event.currentTarget as HTMLElement).closest('details')
  if (!(details instanceof HTMLDetailsElement)) return
  const nextOpen = !details.open
  if (nextOpen) closeOtherUsageRows(details)
  details.open = nextOpen
}

async function handleDateRangeChange() {
  await resetAndReload()
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
    <div v-loading="loading && usagePage.items.length > 0" class="user-panel usage-console-panel">
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
          <el-button :icon="Download" :disabled="usagePage.items.length === 0" @click="exportUsage">
            {{ t('exportDetails') }}
          </el-button>
          <el-tooltip :content="t('refresh')" placement="top">
            <el-button :icon="Refresh" :loading="loading" @click="reload" />
          </el-tooltip>
        </div>
      </div>

      <div class="usage-list">
        <div v-if="usagePage.items.length > 0" class="usage-table-header" role="row">
          <span>{{ t('time') }}</span>
          <span>{{ t('model') }}</span>
          <span>{{ t('tokensColumnHint') }}</span>
          <span>{{ t('latencyColumnHint') }}</span>
          <span>{{ t('cost') }}</span>
          <span>{{ t('actions') }}</span>
        </div>
        <details
          v-for="row in usagePage.items"
          :key="row.id"
          class="usage-row"
        >
          <summary @click.prevent>
            <span class="usage-time">{{ formatFullTime(row.created_at) }}</span>
            <span class="usage-model-cell">
              <span class="usage-model-pill">
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
            <button class="usage-details-label" type="button" @click="toggleUsageDetails">
              {{ t('viewDetails') }}
            </button>
          </summary>
          <div class="usage-detail-panel">
            <section class="usage-detail-section">
              <h4>调用信息</h4>
              <dl class="usage-detail-list">
                <div>
                  <dt>{{ t('model') }}</dt>
                  <dd>{{ row.model || '-' }}</dd>
                </div>
                <div>
                  <dt>{{ t('time') }}</dt>
                  <dd>{{ formatClockTime(row.created_at) }}</dd>
                </div>
                <div>
                  <dt>{{ t('responseMode') }}</dt>
                  <dd>{{ row.streamed ? t('streamLabel') : t('nonStreamLabel') }}</dd>
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
              </dl>
            </section>
            <section v-if="row.routing" class="usage-detail-section usage-routing-section">
              <h4>{{ t('autoRouting') }}</h4>
              <dl class="usage-detail-list usage-routing-list">
                <div>
                  <dt>{{ t('model') }}</dt>
                  <dd>{{ row.routing.requested_model }} -> {{ row.routing.selected_model }}</dd>
                </div>
                <div>
                  <dt>{{ t('routingTier') }}</dt>
                  <dd>{{ routingTierLabel(row.routing.tier) }}</dd>
                </div>
                <div>
                  <dt>{{ t('routingTask') }}</dt>
                  <dd>{{ routingTaskText(row.routing.task_type) }}</dd>
                </div>
                <div v-if="row.routing.candidate_summary.length">
                  <dt>{{ t('routingCandidates') }}</dt>
                  <dd>{{ routingCandidatesText(row) }}</dd>
                </div>
                <div class="usage-routing-wide">
                  <dt>{{ t('routingReason') }}</dt>
                  <dd>{{ routingReasonText(row) }}</dd>
                </div>
                <div v-if="row.routing.matched_rule_ids.length" class="usage-routing-wide">
                  <dt>{{ t('routingRules') }}</dt>
                  <dd>{{ routingRulesText(row) }}</dd>
                </div>
              </dl>
            </section>
            <section class="usage-detail-section">
              <h4>用量</h4>
              <dl class="usage-detail-list">
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
              </dl>
            </section>
            <section class="usage-detail-section">
              <h4>性能</h4>
              <dl class="usage-detail-list">
                <div>
                  <dt>{{ t('totalLatencyDetail') }}</dt>
                  <dd>{{ formatDurationMs(row.latency_ms) }}</dd>
                </div>
                <div>
                  <dt>{{ t('firstResponseLatencyDetail') }}</dt>
                  <dd>{{ formatDurationMs(row.first_response_ms) }}</dd>
                </div>
                <div>
                  <dt>{{ t('throughput') }}</dt>
                  <dd>{{ formatTokenRate(row.output_tokens_per_second, locale) }}</dd>
                </div>
              </dl>
            </section>
            <section class="usage-detail-section">
              <h4>计费</h4>
              <dl class="usage-detail-list">
                <div>
                  <dt>{{ t('cost') }}</dt>
                  <dd>{{ formatMicroUsd(row.cost_micro_usd, 6) }}</dd>
                </div>
                <div>
                  <dt>{{ t('billingStatus') }}</dt>
                  <dd><span class="usage-detail-tag">{{ row.billing_status || '-' }}</span></dd>
                </div>
              </dl>
            </section>
            <section v-if="row.error_summary" class="usage-detail-section">
              <h4>错误说明</h4>
              <dl class="usage-detail-list">
                <div class="usage-detail-wide">
                  <dt>{{ t('errorSummary') }}</dt>
                  <dd class="is-error">{{ row.error_summary }}</dd>
                </div>
              </dl>
            </section>
          </div>
        </details>
        <div
          v-if="loading && usagePage.items.length === 0"
          class="usage-loading-rows"
          aria-hidden="true"
        >
          <span v-for="index in loadingRowCount" :key="index">
            <i></i>
            <i></i>
            <i></i>
          </span>
        </div>
        <div v-else-if="usagePage.items.length === 0" class="usage-empty-state">
          <el-empty :description="t('noData')" />
        </div>
      </div>
    </div>

    <div
      v-if="!usageInitialLoading && (hasUsagePagination || usagePage.items.length > 1)"
      class="admin-pagination-bar"
    >
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
  font-weight: 650;
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
  background: var(--skeleton-gradient);
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
  font-size: 13px;
  font-weight: 400;
}

.usage-time {
  color: #475467;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-size: 12.5px;
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
  display: inline-flex;
  gap: 2px;
  justify-self: start;
  max-width: 100%;
  min-height: 30px;
}

.usage-model-pill > span {
  color: #111827;
  font-size: 13.5px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-token-stack > span {
  color: #111827;
  font-size: 13.5px;
  font-weight: 500;
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
  gap: 4px;
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
  min-height: 21px;
  padding: 0 6px;
}

.usage-cost {
  color: #111827;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-size: 13px;
  font-weight: 400;
  text-align: left;
}

.usage-status {
  align-items: center;
  background: #ecfdf3;
  border: 1px solid #bbf7d0;
  border-radius: 7px;
  color: #047857;
  display: inline-flex;
  font-size: 12px;
  font-weight: 500;
  justify-self: start;
  line-height: 1;
  min-height: 21px;
  padding: 0 8px;
}

.usage-status.is-error {
  background: #fef2f2;
  border-color: #fecaca;
  color: #b42318;
}

.usage-details-label {
  appearance: none;
  background: transparent;
  border: 0;
  color: var(--user-primary, #168bd3);
  cursor: pointer;
  font-size: 12.5px;
  font-weight: 500;
  padding: 0;
  text-align: left;
  white-space: nowrap;
}

.usage-details-label:focus-visible {
  border-radius: 6px;
  box-shadow: 0 0 0 3px rgba(22, 139, 211, 0.16);
  outline: none;
}

.usage-detail-panel {
  background: #ffffff;
  border-top: 1px solid #f4f7fb;
  display: grid;
  gap: 11px;
  grid-template-columns: minmax(0, 1fr);
  padding: 14px 22px 16px 56px;
}

.usage-detail-section {
  display: grid;
  gap: 9px;
  min-width: 0;
}

.usage-detail-section + .usage-detail-section {
  border-top: 1px solid #f4f7fb;
  padding-top: 11px;
}

.usage-detail-section h4 {
  color: #475467;
  font-size: 12px;
  font-weight: 650;
  margin: 0;
}

.usage-routing-section {
  gap: 8px;
}

.usage-routing-list {
  gap: 8px 28px;
  grid-template-columns:
    minmax(220px, max-content) minmax(120px, max-content) minmax(150px, max-content)
    minmax(180px, 1fr);
}

.usage-routing-list .usage-routing-wide {
  grid-column: 1 / -1;
}

.usage-detail-list {
  display: grid;
  gap: 7px 14px;
  grid-template-columns: repeat(auto-fit, minmax(156px, max-content));
  margin: 0;
  min-width: 0;
}

.usage-detail-list div {
  align-items: center;
  display: inline-flex;
  gap: 8px;
  min-width: 0;
}

.usage-detail-list dt {
  flex: 0 0 auto;
  color: #8a95a5;
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
}

.usage-detail-list dd {
  color: #111827;
  flex: 1 1 auto;
  font-size: 13px;
  font-weight: 400;
  margin: 0;
  min-width: 0;
  overflow-wrap: anywhere;
}

.usage-detail-list .usage-detail-wide {
  grid-column: 1 / -1;
  min-width: 0;
}

.usage-detail-list dd.is-error {
  background: #fef2f2;
  border: 1px solid #fecaca;
  border-radius: 7px;
  color: #b42318;
  padding: 8px 10px;
}

.usage-detail-tag {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #dbe3ee;
  border-radius: 7px;
  color: #475467;
  display: inline-flex;
  font-size: 12px;
  font-weight: 500;
  line-height: 1;
  min-height: 21px;
  padding: 0 8px;
}

@media (max-width: 820px) {
  .usage-table-header {
    display: none;
  }

  .usage-detail-panel {
    padding: 14px 16px;
  }

  .usage-detail-section {
    gap: 9px;
  }

  .usage-routing-list {
    grid-template-columns: 1fr;
  }

  .usage-routing-list .usage-routing-wide {
    grid-column: auto;
  }

  .usage-detail-list {
    gap: 8px;
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
