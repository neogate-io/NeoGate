<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useRoute } from 'vue-router'
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Download,
  Refresh,
  Search,
  WarningFilled
} from '@element-plus/icons-vue'
import {
  downloadAdminUsageCsv,
  getAdminUsage,
  type AdminUsageQuery,
  type AdminUsageStatus,
  type UsagePage
} from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useBillingCurrency } from '../../composables/useBillingCurrency'
import { useCursorPageActions } from '../../composables/useCursorPageActions'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import type { UsageRecord } from '../../types/admin'
import {
  cacheWriteTokens,
  downloadBlob,
  formatDateTime,
  formatDurationMs,
  formatNumber,
  formatTokenRate
} from '../../utils/format'

const { locale, t } = useLocale()
const { formatMoney, formatPricePerMillion } = useBillingCurrency()
const route = useRoute()

const DEFAULT_PAGE_SIZE = 20

type UsageFilters = {
  dateRange: string[] | null
  query: string
  status: AdminUsageStatus
}

const filters = reactive<UsageFilters>({
  dateRange: initialRouteDateRange(),
  query: '',
  status: 'all'
})
const exporting = ref(false)
const {
  currentPage,
  pageSize,
  currentCursor,
  reset: resetCursorPagination,
  goToNext,
  goToPrevious
} = useCursorPagination(DEFAULT_PAGE_SIZE)

const usageQueryRange = computed(() => {
  const [start, end] = filters.dateRange ?? []
  return {
    start: start ? new Date(`${start}T00:00:00`).toISOString() : undefined,
    end: end ? new Date(`${end}T23:59:59.999`).toISOString() : undefined
  }
})
const routeUsageContext = computed<AdminUsageQuery>(() => ({
  project_id: numberQueryValue('project_id'),
  user_id: numberQueryValue('user_id'),
  user_key_id: numberQueryValue('user_key_id'),
  channel_id: numberQueryValue('channel_id'),
  model: stringQueryValue('model'),
  billing_meter: billingMeterQueryValue()
}))
const usageBaseQuery = computed<AdminUsageQuery>(() => ({
  start: usageQueryRange.value.start,
  end: usageQueryRange.value.end,
  query: filters.query.trim() || undefined,
  status: filters.status,
  ...routeUsageContext.value
}))

const {
  data: usagePage,
  loading,
  loaded: usageLoaded,
  reload
} = useAsyncData(
  () =>
    getAdminUsage({
      ...usageBaseQuery.value,
      page: currentPage.value,
      limit: pageSize.value,
      cursor: currentCursor.value
    }),
  { items: [], total: 0, page: 1, limit: DEFAULT_PAGE_SIZE } satisfies UsagePage
)

const usageInitialLoading = computed(() => !usageLoaded.value)
const hasUsagePagination = computed(
  () => currentPage.value > 1 || Boolean(usagePage.value.has_more)
)
const {
  resetAndReload: resetUsageAndReload,
  nextPage,
  previousPage,
  handlePageSizeChange
} = useCursorPageActions(
  { pageSize, reset: resetCursorPagination, goToNext, goToPrevious },
  () => usagePage.value,
  reload
)

function usageStatusTone(statusCode?: number | null) {
  if (statusCode == null) return 'neutral'
  if (statusCode >= 200 && statusCode < 400) return 'success'
  return 'danger'
}

function usageStatusIcon(statusCode?: number | null) {
  return statusCode != null && statusCode >= 200 && statusCode < 400
    ? CircleCheckFilled
    : WarningFilled
}

function usageStatusLabel(statusCode?: number | null) {
  if (statusCode == null) return t('usageStatusUnknown')
  return statusCode >= 200 && statusCode < 400 ? t('usageStatusSuccess') : t('usageStatusFailed')
}

function usageStatusTooltip(statusCode?: number | null) {
  return statusCode == null ? '' : `HTTP ${statusCode}`
}

function relayPathSegments(row: UsageRecord): string[] {
  if (!row.relay_path) {
    const label = relayChannelLabel(row)
    return label === '-' ? [] : [label]
  }
  return row.relay_path.split(' → ')
}

function relayTraceTone(row: UsageRecord) {
  if (!row.relay_trace_id) return 'neutral'
  return row.relay_final ? 'success' : 'warning'
}

function relayChannelLabel(row: UsageRecord) {
  if (row.channel_id == null) return '-'
  return `#${row.channel_id}`
}

function usageUserDisplay(row: UsageRecord) {
  if (row.user_username) return row.user_username
  if (row.user_email) return row.user_email
  if (row.user_id != null) return `#${row.user_id}`
  return '-'
}

function usageModelDisplay(row: UsageRecord) {
  const model = row.model?.trim()
  const upstreamModel = row.upstream_model?.trim()
  if (model && upstreamModel && model !== upstreamModel) return `${model} / ${upstreamModel}`
  return model || upstreamModel || '-'
}

function routingTierLabel(tier?: string | null) {
  if (tier === 'simple') return t('routingTier_simple')
  if (tier === 'standard') return t('routingTier_standard')
  if (tier === 'advanced') return t('routingTier_advanced')
  return tier || '-'
}

function routingReasonText(row: UsageRecord) {
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

function initialRouteDateRange() {
  const start = stringQueryValue('start')
  const end = stringQueryValue('end')
  return start && end ? [start, end] : []
}

function stringQueryValue(key: string) {
  const value = route.query[key]
  return Array.isArray(value) ? (value[0] ?? undefined) : (value ?? undefined)
}

function numberQueryValue(key: string) {
  const value = stringQueryValue(key)
  if (!value) return undefined
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : undefined
}

function billingMeterQueryValue() {
  const value = stringQueryValue('billing_meter')
  return value === 'token' || value === 'image' || value === 'video' ? value : undefined
}

function routingRulesText(row: UsageRecord) {
  return (
    row.routing?.matched_rule_ids
      .map(routingRuleText)
      .join(locale.value === 'zh-CN' ? '；' : '; ') || ''
  )
}

function routingCandidatesText(row: UsageRecord) {
  return (
    row.routing?.candidate_summary
      .map((candidate) => `${candidate.target_model} P${candidate.priority}/W${candidate.weight}`)
      .join(locale.value === 'zh-CN' ? '；' : '; ') || ''
  )
}

function usageIsFailed(row: UsageRecord) {
  return Boolean(row.error_summary) || (row.status_code != null && row.status_code >= 400)
}

function usageDetailSummary(row: UsageRecord) {
  if (usageIsFailed(row)) {
    return compactUsageError(row)
  }

  return usageSuccessSummary(row)
}

function usageSuccessSummary(row: UsageRecord) {
  if (row.billing_meter === 'image') {
    return `${t('usageDetailImageGeneration')} · ${formatNumber(row.billable_units, locale.value)} ${t('usageDetailImages')}`
  }

  if (row.billing_meter === 'video') {
    const parts = [t('usageDetailVideoGeneration')]
    const isPerSecondBilling =
      row.billable_units > 0 && row.billable_units !== (row.total_tokens ?? 0)
    if (isPerSecondBilling) {
      parts.push(`${formatNumber(row.billable_units, locale.value)} ${t('usageDetailSeconds')}`)
    } else if ((row.total_tokens ?? 0) > 0) {
      parts.push(`${formatNumber(row.total_tokens, locale.value)} ${t('usageDetailTokens')}`)
    }
    if (parts.length === 1 && row.billable_units > 0) {
      parts.push(`${formatNumber(row.billable_units, locale.value)} ${t('usageDetailSeconds')}`)
    }
    return parts.join(' · ')
  }

  if ((row.total_tokens ?? 0) > 0) {
    return `${t('usageDetailTextCall')} · ${formatNumber(row.total_tokens, locale.value)} ${t('usageDetailTokens')}`
  }

  if (row.billable_units > 0) {
    return `${t('usageDetailCompleted')} · ${formatNumber(row.billable_units, locale.value)} ${t('usageDetailBillingUnits')}`
  }

  return t('usageDetailCompleted')
}

function compactUsageError(row: UsageRecord) {
  const error = row.error_summary?.trim()
  if (!error) return t('usageStatusFailed')

  const lower = error.toLowerCase()
  if (/upstream model unavailable|invalidendpointormodel|not\s*found/.test(lower)) {
    return t('usageErrorUpstreamModelUnavailable')
  }
  if (/timeout|timed out|deadline/.test(lower)) return t('usageErrorUpstreamTimeout')
  if (/insufficient|quota|balance|credit/.test(lower)) return t('usageErrorInsufficientCredit')
  if (/unauthorized|forbidden|authentication|permission|access denied/.test(lower)) {
    return t('usageErrorAuthFailed')
  }

  let summary = error
    .split(/\bRequest id:/i)[0]
    .split(/;\s*type=/i)[0]
    .split(/;\s*code=/i)[0]
    .replace(/^upstream [^:]+:\s*/i, '')
    .replace(/\bstatus\s+\d{3};?\s*/i, '')
    .replace(/;\s*$/, '')
    .trim()

  if (/^status\s+\d{3};?/i.test(summary)) {
    summary = t('usageStatusFailed')
  }
  summary = summary.replace(/\s+/g, ' ')

  return summary || t('usageStatusFailed')
}

function usageBillingUnitsDisplay(row: UsageRecord) {
  const value = formatNumber(row.billable_units, locale.value)
  if (row.billing_meter === 'video') {
    const isPerSecondBilling =
      row.billable_units > 0 && row.billable_units !== (row.total_tokens ?? 0)
    return `${value} ${isPerSecondBilling ? t('usageDetailSeconds') : t('usageDetailTokens')}`
  }
  if (row.billing_meter === 'image') return `${value} ${t('usageDetailImages')}`
  return `${value} ${t('usageDetailBillingUnits')}`
}

function videoBillingDetailsAvailable(row: UsageRecord) {
  return (
    row.billing_meter === 'video' &&
    !usageIsFailed(row) &&
    (row.billing_status === 'billed' || row.billing_status === 'undercharged') &&
    row.video_billing != null &&
    row.cost_micros != null
  )
}

function videoBillingRuleDisplay(row: UsageRecord) {
  const details = row.video_billing
  if (!details) return '-'
  if (details.mode === 'per_second') {
    return `${t('videoBillingPerSecond')} · ${formatMoney(details.price_micros, locale.value)} / ${t('perSecond')}`
  }
  return `${t('videoBillingByToken')} · ${formatPricePerMillion(details.price_micros, locale.value)} / ${t('perMillionTokens')}`
}

function videoBillingFormulaDisplay(row: UsageRecord) {
  const details = row.video_billing
  if (!details || row.cost_micros == null) return '-'
  if (details.mode === 'per_second') {
    return `${formatNumber(row.billable_units, locale.value)} ${t('usageDetailSeconds')} × ${formatMoney(details.price_micros, locale.value)} = ${formatMoney(row.cost_micros, locale.value)}`
  }
  return `${formatNumber(row.billable_units, locale.value)} ${t('usageDetailTokens')} × ${formatPricePerMillion(details.price_micros, locale.value)} / ${t('perMillionTokens')} = ${formatMoney(row.cost_micros, locale.value)}`
}

function usageDetailRows(row: UsageRecord) {
  const rows = [{ label: t('usageDetailsColumn'), value: usageDetailSummary(row) }]
  if (row.error_summary) rows.push({ label: t('errorSummary'), value: row.error_summary })
  if (row.status_code != null) rows.push({ label: t('status'), value: `HTTP ${row.status_code}` })
  rows.push({ label: t('model'), value: usageModelDisplay(row) })
  if (
    row.billing_meter === 'token' &&
    ((row.input_tokens ?? 0) > 0 || (row.output_tokens ?? 0) > 0)
  ) {
    rows.push({
      label: t('usageDetailTokens'),
      value: `${formatNumber(row.input_tokens, locale.value)} / ${formatNumber(row.output_tokens, locale.value)}`
    })
  }
  if (row.billing_meter !== 'token' && row.billable_units > 0) {
    rows.push({ label: t('billingUnits'), value: usageBillingUnitsDisplay(row) })
  }
  if (videoBillingDetailsAvailable(row)) {
    const details = row.video_billing!
    rows.push({ label: t('videoResolution'), value: details.resolution })
    rows.push({
      label: t('videoInput'),
      value: details.has_video_input ? t('videoInputWithVideo') : t('videoInputWithoutVideo')
    })
    rows.push({ label: t('videoBillingRule'), value: videoBillingRuleDisplay(row) })
    rows.push({ label: t('videoBillingFormula'), value: videoBillingFormulaDisplay(row) })
    if (details.mode === 'per_second' && (row.total_tokens ?? 0) > 0) {
      rows.push({
        label: t('upstreamUsage'),
        value: `${formatNumber(row.total_tokens, locale.value)} ${t('usageDetailTokens')} · ${t('upstreamUsageNotBilled')}`
      })
    }
  }
  rows.push({ label: t('relayTipLatency'), value: formatDurationMs(row.latency_ms) })
  if (row.channel_name || row.channel_id != null) {
    rows.push({
      label: t('relayTipPath'),
      value: `${row.relay_path || relayChannelLabel(row)} ${row.channel_name || ''}`.trim()
    })
  }
  if (!row.relay_final) {
    rows.push({ label: t('relayTrace'), value: t('relayTooltipRetry') })
  }
  if (row.routing) {
    rows.push({
      label: t('autoRouting'),
      value: `${row.routing.requested_model} -> ${row.routing.selected_model}`
    })
    rows.push({ label: t('routingTier'), value: routingTierLabel(row.routing.tier) })
    rows.push({ label: t('routingTask'), value: routingTaskText(row.routing.task_type) })
    rows.push({ label: t('routingReason'), value: routingReasonText(row) })
    if (row.routing.matched_rule_ids.length) {
      rows.push({ label: t('routingRules'), value: routingRulesText(row) })
    }
    if (row.routing.candidate_summary.length) {
      rows.push({ label: t('routingCandidates'), value: routingCandidatesText(row) })
    }
  }
  return rows
}

async function handleSearch() {
  await resetUsageAndReload()
}

async function exportUsage() {
  exporting.value = true
  try {
    const result = await downloadAdminUsageCsv(usageBaseQuery.value)
    downloadBlob(
      result.filename ?? `usage-details-${new Date().toISOString().slice(0, 10)}.csv`,
      result.blob
    )
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <section class="grid usage-view">
    <el-form class="usage-toolbar" @submit.prevent="handleSearch">
      <div class="usage-toolbar-filters">
        <label class="admin-filter-field">
          <span>{{ t('timeRange') }}</span>
          <el-date-picker
            v-model="filters.dateRange"
            class="usage-date-range"
            type="daterange"
            value-format="YYYY-MM-DD"
            :range-separator="t('to')"
            :start-placeholder="t('startTime')"
            :end-placeholder="t('endTime')"
          />
        </label>
        <label class="admin-filter-field">
          <span>{{ t('channelOrModel') }}</span>
          <el-input
            v-model="filters.query"
            class="usage-search-input"
            clearable
            :prefix-icon="Search"
            :placeholder="t('usageModelSearchPlaceholder')"
          />
        </label>
        <label class="admin-filter-field">
          <span>{{ t('status') }}</span>
          <el-select v-model="filters.status" class="usage-status-filter">
            <el-option :label="t('usageStatusAll')" value="all" />
            <el-option :label="t('usageStatusSuccess')" value="success" />
            <el-option :label="t('usageStatusFailed')" value="failed" />
          </el-select>
        </label>
        <el-button
          class="admin-action-button"
          type="primary"
          native-type="submit"
          :icon="Search"
          :loading="loading"
        >
          {{ t('search') }}
        </el-button>
      </div>
      <div class="usage-toolbar-actions">
        <el-button
          class="admin-action-button"
          :icon="Download"
          :loading="exporting"
          :disabled="usagePage.items.length === 0"
          @click="exportUsage"
        >
          {{ t('exportDetails') }}
        </el-button>
        <el-button class="admin-action-button" :icon="Refresh" :loading="loading" @click="reload">
          {{ t('refresh') }}
        </el-button>
      </div>
    </el-form>

    <div
      v-if="usageInitialLoading"
      v-loading="true"
      class="service-table-panel usage-table-loading"
    >
      <div class="usage-table-loading-head">
        <span></span>
        <span></span>
        <span></span>
        <span></span>
        <span></span>
        <span></span>
      </div>
      <div class="usage-table-loading-row"></div>
      <div class="usage-table-loading-row"></div>
      <div class="usage-table-loading-row"></div>
    </div>

    <div
      v-else
      class="service-table-panel"
      :class="{ 'has-pagination': hasUsagePagination || usagePage.items.length > 1 }"
    >
      <el-table
        v-loading="loading"
        class="admin-table service-table usage-table"
        :data="usagePage.items"
        row-key="id"
        stripe
      >
        <el-table-column :label="t('time')" min-width="160">
          <template #default="{ row }">
            <span class="usage-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('usageUser')" min-width="110">
          <template #default="{ row }">
            <span
              class="usage-user-cell"
              :class="{ 'is-empty': !row.user_username && !row.user_email && !row.user_id }"
            >
              {{ usageUserDisplay(row) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('model')" min-width="220">
          <template #default="{ row }">
            <span class="usage-model-name">{{ usageModelDisplay(row) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('latencyColumnHint')" min-width="140">
          <template #default="{ row }">
            <div class="usage-stack">
              <div class="usage-tags">
                <span class="usage-latency-tag">{{ formatDurationMs(row.latency_ms) }}</span>
                <span v-if="row.first_response_ms != null" class="usage-latency-tag">
                  {{ formatDurationMs(row.first_response_ms) }}
                </span>
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
        <el-table-column :label="t('tokensColumnHint')" min-width="150">
          <template #default="{ row }">
            <div v-if="usageIsFailed(row)" class="usage-stack">
              <span class="usage-mono">-</span>
            </div>
            <div v-else-if="row.billing_meter === 'image'" class="usage-stack">
              <span class="usage-mono">
                {{ formatNumber(row.billable_units, locale) }} {{ t('perImage') }}
              </span>
              <span class="usage-muted">{{ t('billingMeterImageGeneration') }}</span>
            </div>
            <div
              v-else-if="row.billing_meter === 'video' && (row.total_tokens ?? 0) === 0"
              class="usage-stack"
            >
              <span class="usage-mono">
                {{ formatNumber(row.billable_units, locale) }} {{ t('perSecond') }}
              </span>
              <span class="usage-muted">{{ t('billingMeterVideo') }}</span>
            </div>
            <div v-else class="usage-stack">
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
        <el-table-column :label="t('cost')" min-width="104" align="right" header-align="right">
          <template #default="{ row }">
            <span class="usage-cost-cell">{{ formatMoney(row.cost_micros, locale, 6) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('status')" min-width="84" align="center" header-align="center">
          <template #default="{ row }">
            <el-tooltip
              :content="usageStatusTooltip(row.status_code)"
              :disabled="row.status_code == null"
              placement="top"
              :show-after="600"
            >
              <span class="usage-status-switch" :class="`is-${usageStatusTone(row.status_code)}`">
                <span class="usage-status-switch-icon">
                  <el-icon><component :is="usageStatusIcon(row.status_code)" /></el-icon>
                </span>
                <span class="usage-status-switch-text">{{
                  usageStatusLabel(row.status_code)
                }}</span>
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column :label="t('relayTrace')" min-width="150" align="left" header-align="left">
          <template #default="{ row }">
            <div class="usage-trace-cell">
              <span class="usage-trace-path" :class="`is-${relayTraceTone(row)}`">
                <template v-for="(seg, i) in relayPathSegments(row)" :key="i">
                  <span v-if="i > 0" class="usage-trace-sep">→</span>
                  <span
                    class="usage-trace-seg"
                    :class="{ 'is-current': i === row.relay_path_index }"
                    >{{ seg }}</span
                  >
                </template>
                <span
                  v-if="!row.relay_trace_id && relayPathSegments(row).length === 0"
                  class="usage-muted"
                  >-</span
                >
              </span>
              <span v-if="row.channel_name" class="usage-trace-name">{{ row.channel_name }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('usageDetailsColumn')" min-width="230">
          <template #default="{ row }">
            <el-tooltip placement="top" :show-after="600" popper-class="usage-detail-tip-popper">
              <template #content>
                <div class="usage-detail-tip">
                  <div
                    v-for="(item, index) in usageDetailRows(row)"
                    :key="index"
                    class="usage-detail-tip-row"
                  >
                    <span class="usage-detail-tip-label">{{ item.label }}</span>
                    <span
                      class="usage-detail-tip-value"
                      :class="{ 'is-error': index === 0 && usageIsFailed(row) }"
                      >{{ item.value }}</span
                    >
                  </div>
                </div>
              </template>
              <span class="usage-detail-cell" :class="{ 'is-error': usageIsFailed(row) }">{{
                usageDetailSummary(row)
              }}</span>
            </el-tooltip>
          </template>
        </el-table-column>
        <template #empty>
          <div class="usage-empty-state">
            <el-empty :description="t('noData')" />
          </div>
        </template>
      </el-table>
    </div>

    <div
      v-if="!usageInitialLoading && (hasUsagePagination || usagePage.items.length > 1)"
      class="admin-pagination-bar admin-table-pagination is-compact"
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
        <div class="admin-page-buttons">
          <el-button
            :aria-label="t('previousPage')"
            :disabled="currentPage <= 1 || loading"
            :icon="ArrowLeft"
            @click="previousPage"
          />
          <span class="admin-page-current">{{ currentPage }}</span>
          <el-button
            :aria-label="t('nextPage')"
            :disabled="!usagePage.has_more || loading"
            :icon="ArrowRight"
            @click="nextPage"
          />
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.usage-table-loading {
  min-height: 236px;
  overflow: hidden;
}

.usage-table-loading-head {
  align-items: center;
  background: #f6f9fc;
  border-bottom: 1px solid #dfe8f2;
  display: grid;
  gap: 30px;
  grid-template-columns: 150px 170px 180px 160px 130px 120px;
  height: 48px;
  min-width: 1180px;
  padding: 0 160px 0 14px;
}

.usage-table-loading-head span,
.usage-table-loading-row::before,
.usage-table-loading-row::after,
.usage-table-loading-row span {
  background: #e8eef6;
  border-radius: 999px;
  content: '';
  display: block;
  height: 12px;
}

.usage-table-loading-head span:nth-child(1) {
  width: 54px;
}

.usage-table-loading-head span:nth-child(2) {
  width: 72px;
}

.usage-table-loading-head span:nth-child(3) {
  width: 58px;
}

.usage-table-loading-head span:nth-child(4) {
  width: 48px;
}

.usage-table-loading-head span:nth-child(5) {
  width: 56px;
}

.usage-table-loading-head span:nth-child(6) {
  width: 54px;
}

.usage-table-loading-row {
  align-items: center;
  border-bottom: 1px solid #edf3f8;
  display: grid;
  gap: 30px;
  grid-template-columns: 150px 170px 180px 160px 130px 120px;
  height: 62px;
  min-width: 1180px;
  padding: 0 160px 0 14px;
}

.usage-table-loading-row::before {
  width: 126px;
}

.usage-table-loading-row::after {
  width: min(240px, 100%);
}

.usage-table-loading-row span {
  width: 78px;
}

.usage-stack {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.usage-muted {
  color: #86909c;
  font-size: 12px;
  font-weight: 560;
}

.usage-model-name {
  color: #1d2129;
  display: block;
  font-size: 13px;
  font-weight: 680;
  line-height: 1.45;
  overflow-wrap: anywhere;
  white-space: normal;
  word-break: normal;
}

.usage-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.usage-table .usage-latency-tag {
  align-items: center;
  animation: none;
  background: var(--admin-success-bg);
  border: 1px solid var(--admin-success-border);
  border-radius: 999px;
  color: var(--admin-success);
  display: inline-flex;
  font-feature-settings: 'tnum';
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-weight: 540;
  height: 22px;
  justify-content: center;
  min-width: 46px;
  padding: 0 7px;
  transition: none;
  white-space: nowrap;
}

.usage-mono {
  color: #1d2939;
  font-feature-settings: 'tnum';
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  font-weight: 400;
}

.usage-time-cell {
  color: #344054;
  font-size: 13px;
  font-weight: 560;
}

.usage-user-cell {
  color: #1d2939;
  display: block;
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-user-cell.is-empty {
  color: #98a2b3;
  font-weight: 520;
}

.usage-cost-cell {
  color: #1d2939;
  font-feature-settings: 'tnum';
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  font-weight: 760;
}

.usage-detail-cell {
  color: #344054;
  display: block;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.5;
  overflow-wrap: anywhere;
  word-break: break-word;
  white-space: normal;
}

.usage-detail-cell.is-error {
  color: #b91c1c;
}

.usage-table .usage-status-switch {
  background: transparent;
  border-color: transparent;
  border-radius: 6px;
  color: #667085;
  gap: 4px;
  min-height: 24px;
  min-width: auto;
  padding: 0 4px;
}

.usage-table .usage-status-switch.is-success {
  background: transparent;
  border-color: transparent;
  color: #24825f;
}

.usage-table .usage-status-switch.is-danger {
  background: transparent;
  border-color: transparent;
  color: #b42318;
}

.usage-table .usage-status-switch.is-neutral {
  background: transparent;
  border-color: transparent;
  color: #667085;
}

.usage-table .usage-status-switch-icon {
  background: transparent;
  color: inherit;
  font-size: 14px;
  height: 16px;
  opacity: 1;
  width: 16px;
}

.usage-table .usage-status-switch-text {
  font-size: 13px;
  font-weight: 560;
}

.usage-trace-cell {
  align-items: flex-start;
  display: inline-flex;
  flex-direction: column;
  gap: 3px;
  max-width: 100%;
}

.usage-trace-path {
  align-items: center;
  border-radius: 6px;
  color: #98a2b3;
  display: inline-flex;
  flex-wrap: wrap;
  gap: 2px;
  justify-content: flex-start;
  line-height: 1.4;
  white-space: nowrap;
}

.usage-trace-name {
  color: #667085;
  display: block;
  font-size: 12px;
  font-weight: 520;
  line-height: 1.25;
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-trace-path.is-success {
  color: #47997a;
}

.usage-trace-path.is-warning {
  color: #9a7a3a;
}

.usage-trace-sep {
  color: #c8ced8;
  font-size: 12px;
  font-weight: 400;
}

.usage-trace-seg {
  font-feature-settings: 'tnum';
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  font-weight: 460;
}

.usage-trace-seg.is-current {
  background: #eef2ff;
  border-radius: 4px;
  color: #1d2939;
  font-weight: 620;
  padding: 0 4px;
}

.usage-trace-path.is-success .usage-trace-seg.is-current {
  background: #e3f6ec;
  color: #067647;
}

.usage-trace-path.is-warning .usage-trace-seg.is-current {
  background: #fdf3e0;
  color: #b54708;
}

.usage-empty-state {
  padding: 30px 0 34px;
}
</style>

<style>
.usage-detail-tip-popper {
  max-width: min(520px, calc(100vw - 48px));
  padding: 4px 2px;
}

.usage-detail-tip {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: min(340px, calc(100vw - 72px));
}

.usage-detail-tip-row {
  align-items: flex-start;
  display: grid;
  gap: 12px;
  grid-template-columns: 86px minmax(0, 1fr);
  line-height: 1.45;
}

.usage-detail-tip-label {
  color: #98a2b3;
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
}

.usage-detail-tip-value {
  color: #f2f4f7;
  font-size: 12px;
  font-weight: 600;
  min-width: 0;
  overflow-wrap: anywhere;
  word-break: normal;
}

.usage-detail-tip-value.is-error {
  color: #fda4af;
}
</style>
