<script setup lang="ts">
import { computed, reactive } from 'vue'
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Refresh,
  Search,
  WarningFilled
} from '@element-plus/icons-vue'
import { getAdminUsage, type AdminUsageStatus, type UsagePage } from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import type { UsageRecord } from '../../types/admin'
import {
  cacheWriteTokens,
  formatDateTime,
  formatDurationMs,
  formatMicroUsd,
  formatNumber,
  formatTokenRate
} from '../../utils/format'

const { locale, t } = useLocale()

const DEFAULT_PAGE_SIZE = 20

type UsageFilters = {
  dateRange: string[] | null
  query: string
  status: AdminUsageStatus
}

const filters = reactive<UsageFilters>({
  dateRange: [],
  query: '',
  status: 'all'
})
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

const {
  data: usagePage,
  loading,
  loaded: usageLoaded,
  reload
} = useAsyncData(
  () =>
    getAdminUsage({
      page: currentPage.value,
      limit: pageSize.value,
      start: usageQueryRange.value.start,
      end: usageQueryRange.value.end,
      query: filters.query.trim() || undefined,
      status: filters.status,
      cursor: currentCursor.value
    }),
  { items: [], total: 0, page: 1, limit: DEFAULT_PAGE_SIZE } satisfies UsagePage
)

const usageItems = computed(() => usagePage.value.items)
const usageInitialLoading = computed(() => !usageLoaded.value)
const hasUsagePagination = computed(
  () => currentPage.value > 1 || Boolean(usagePage.value.has_more)
)

function resetUsagePagination() {
  resetCursorPagination()
}

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

function usageUserDisplay(row: UsageRecord) {
  if (row.user_email) return row.user_email
  if (row.user_id != null) return `#${row.user_id}`
  return '-'
}

async function handleSearch() {
  resetUsagePagination()
  await reload()
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
  resetUsagePagination()
  await reload()
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
          <span>{{ t('providerOrModel') }}</span>
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
      :class="{ 'has-pagination': hasUsagePagination || usageItems.length > 1 }"
    >
      <el-table
        v-loading="loading"
        class="admin-table service-table usage-table"
        :data="usageItems"
        row-key="id"
        stripe
      >
        <el-table-column :label="t('time')" min-width="180">
          <template #default="{ row }">
            <span class="usage-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('usageUser')" min-width="220">
          <template #default="{ row }">
            <span class="usage-user-cell" :class="{ 'is-empty': !row.user_email && !row.user_id }">
              {{ usageUserDisplay(row) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('model')" min-width="190">
          <template #default="{ row }">
            <div class="usage-model">
              <span class="usage-provider">{{ row.provider }}</span>
              <span>{{ row.model || '-' }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('latencyColumnHint')" min-width="170">
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
        <el-table-column :label="t('cost')" min-width="130" align="right" header-align="right">
          <template #default="{ row }">
            <span class="usage-cost-cell">{{ formatMicroUsd(row.cost_micro_usd, 6) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('status')" min-width="120" align="center" header-align="center">
          <template #default="{ row }">
            <el-tooltip
              :content="usageStatusTooltip(row.status_code)"
              :disabled="row.status_code == null"
              placement="top"
            >
              <span
                class="channel-runtime-status usage-status-tag"
                :class="`is-${usageStatusTone(row.status_code)}`"
              >
                <el-icon><component :is="usageStatusIcon(row.status_code)" /></el-icon>
                {{ usageStatusLabel(row.status_code) }}
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column prop="error_summary" :label="t('error')" min-width="180">
          <template #default="{ row }">
            <el-tooltip v-if="row.error_summary" :content="row.error_summary" placement="top">
              <span class="usage-error-cell">{{ row.error_summary }}</span>
            </el-tooltip>
            <span v-else class="usage-muted">-</span>
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
      v-if="!usageInitialLoading && (hasUsagePagination || usageItems.length > 1)"
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
        <span class="admin-result-count">{{ t('currentPage') }} {{ currentPage }}</span>
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

.usage-model,
.usage-stack {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.usage-provider,
.usage-muted {
  color: #86909c;
  font-size: 12px;
  font-weight: 560;
}

.usage-model > span:last-child {
  color: #1d2129;
  font-size: 14px;
  font-weight: 680;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.usage-table .usage-latency-tag {
  align-items: center;
  animation: none;
  background: #eef8f2;
  border: 1px solid #d7eadf;
  border-radius: 999px;
  color: #3f7a55;
  display: inline-flex;
  font-feature-settings: 'tnum';
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-weight: 560;
  height: 24px;
  justify-content: center;
  min-width: 58px;
  padding: 0 10px;
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

.usage-error-cell {
  color: #b91c1c;
  display: block;
  font-size: 13px;
  font-weight: 560;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-empty-state {
  padding: 30px 0 34px;
}
</style>
