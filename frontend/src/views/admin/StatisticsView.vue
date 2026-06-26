<script setup lang="ts">
import { computed, defineAsyncComponent, reactive, ref } from 'vue'
import type { EChartsCoreOption } from 'echarts/core'
import { Download, Search } from '@element-plus/icons-vue'
import {
  downloadAdminUsageStatisticsCsv,
  getAdminUsageStatisticsModels,
  getAdminUsageStatisticsOptions,
  getAdminUsageStatisticsSummary,
  getAdminUsageStatisticsUsers,
  type ModelUsageStatistics,
  type UsageStatisticsExportScope,
  type UsageStatisticsPage,
  type UsageStatisticsQuery,
  type UsageStatisticsSummary,
  type UserUsageStatistics
} from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import {
  downloadBlob,
  formatDurationMs,
  formatMicroUsd,
  formatNumber,
  microUsdToUsd,
  toDateKey
} from '../../utils/format'

const { locale, t } = useLocale()
const AdminUsageChart = defineAsyncComponent(
  () => import('../../components/admin/common/AdminUsageChart.vue')
)

type StatisticsFilters = {
  dateRange: string[] | null
  userQuery: string
  provider: string
  model: string
  billingMeter: '' | 'token' | 'image'
}

const statisticsFilters = reactive<StatisticsFilters>({
  dateRange: defaultStatisticsRange(30),
  userQuery: '',
  provider: '',
  model: '',
  billingMeter: ''
})
const statisticsUsersPage = ref(1)
const statisticsUsersPageSize = ref(20)
const statisticsModelsPage = ref(1)
const statisticsModelsPageSize = ref(20)
const statisticsExporting = ref(false)

const statisticsBaseQuery = computed<UsageStatisticsQuery>(() => {
  const [start, end] = statisticsFilters.dateRange ?? []
  return {
    start,
    end,
    user_query: statisticsFilters.userQuery.trim() || undefined,
    provider: statisticsFilters.provider || undefined,
    model: statisticsFilters.model || undefined,
    billing_meter: statisticsFilters.billingMeter || undefined,
    sort: 'cost_desc'
  }
})

const {
  data: statisticsSummary,
  loading: statisticsSummaryLoading,
  loaded: statisticsSummaryLoaded,
  reload: reloadStatisticsSummary
} = useAsyncData(
  () => getAdminUsageStatisticsSummary(statisticsBaseQuery.value),
  emptyStatisticsSummary()
)
const {
  data: statisticsUsers,
  loading: statisticsUsersLoading,
  reload: reloadStatisticsUsers
} = useAsyncData(
  () =>
    getAdminUsageStatisticsUsers({
      ...statisticsBaseQuery.value,
      page: statisticsUsersPage.value,
      limit: statisticsUsersPageSize.value
    }),
  emptyStatisticsPage<UserUsageStatistics>()
)
const {
  data: statisticsModels,
  loading: statisticsModelsLoading,
  reload: reloadStatisticsModels
} = useAsyncData(
  () =>
    getAdminUsageStatisticsModels({
      ...statisticsBaseQuery.value,
      page: statisticsModelsPage.value,
      limit: statisticsModelsPageSize.value
    }),
  emptyStatisticsPage<ModelUsageStatistics>()
)
const {
  data: statisticsOptions,
  loading: statisticsOptionsLoading,
  reload: reloadStatisticsOptions
} = useAsyncData(
  () => getAdminUsageStatisticsOptions(statisticsBaseQuery.value),
  { providers: [], models: [], users: [] }
)

const statisticsLoading = computed(
  () =>
    statisticsSummaryLoading.value ||
    statisticsUsersLoading.value ||
    statisticsModelsLoading.value ||
    statisticsOptionsLoading.value
)
const statisticsInitialLoading = computed(() => !statisticsSummaryLoaded.value)
const statisticsEmpty = computed(() => statisticsSummary.value.totals.request_count === 0)
const filteredModelOptions = computed(() =>
  statisticsOptions.value.models.filter(
    (item) => !statisticsFilters.provider || item.provider === statisticsFilters.provider
  )
)
const dailyChartRows = computed(() => filledDailyRows(statisticsSummary.value))
const costTrendOption = computed<EChartsCoreOption>(() => ({
  color: ['#2563eb'],
  grid: { left: 12, right: 18, top: 28, bottom: 28, containLabel: true },
  tooltip: {
    trigger: 'axis',
    formatter: (params: unknown) => trendTooltip(params, t('cost'), 'cost')
  },
  xAxis: {
    type: 'category',
    data: dailyChartRows.value.map((item) => item.date),
    axisLabel: { color: '#667085', hideOverlap: true }
  },
  yAxis: {
    type: 'value',
    axisLabel: { color: '#667085', formatter: (value: number) => `$${value}` },
    splitLine: { lineStyle: { color: '#edf2f7' } }
  },
  series: [
    {
      name: t('cost'),
      type: 'line',
      smooth: true,
      areaStyle: { opacity: 0.12 },
      data: dailyChartRows.value.map((item) => Number(microUsdToUsd(item.cost_micro_usd).toFixed(6)))
    }
  ]
}))
const requestTrendOption = computed<EChartsCoreOption>(() => ({
  color: ['#16a34a'],
  grid: { left: 12, right: 18, top: 28, bottom: 28, containLabel: true },
  tooltip: {
    trigger: 'axis',
    formatter: (params: unknown) => trendTooltip(params, t('requestCount'), 'number')
  },
  xAxis: {
    type: 'category',
    data: dailyChartRows.value.map((item) => item.date),
    axisLabel: { color: '#667085', hideOverlap: true }
  },
  yAxis: {
    type: 'value',
    axisLabel: { color: '#667085' },
    splitLine: { lineStyle: { color: '#edf2f7' } }
  },
  series: [
    {
      name: t('requestCount'),
      type: 'bar',
      barMaxWidth: 20,
      data: dailyChartRows.value.map((item) => item.request_count)
    }
  ]
}))
const topUsersOption = computed<EChartsCoreOption>(() => {
  const rows = [...statisticsSummary.value.top_users].reverse()
  return {
    color: ['#0f766e'],
    grid: { left: 12, right: 24, top: 18, bottom: 24, containLabel: true },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      formatter: (params: unknown) => trendTooltip(params, t('cost'), 'cost')
    },
    xAxis: {
      type: 'value',
      axisLabel: { color: '#667085', formatter: (value: number) => `$${value}` },
      splitLine: { lineStyle: { color: '#edf2f7' } }
    },
    yAxis: {
      type: 'category',
      data: rows.map((item) => item.user_display_name),
      axisLabel: { color: '#667085', width: 110, overflow: 'truncate' }
    },
    series: [
      {
        name: t('cost'),
        type: 'bar',
        barMaxWidth: 16,
        data: rows.map((item) => Number(microUsdToUsd(item.cost_micro_usd).toFixed(6)))
      }
    ]
  }
})
const topModelsOption = computed<EChartsCoreOption>(() => ({
  color: ['#7c3aed'],
  grid: { left: 12, right: 18, top: 28, bottom: 54, containLabel: true },
  tooltip: {
    trigger: 'axis',
    axisPointer: { type: 'shadow' },
    formatter: (params: unknown) => trendTooltip(params, t('cost'), 'cost')
  },
  xAxis: {
    type: 'category',
    data: statisticsSummary.value.top_models.map((item) => modelDisplay(item.provider, item.model)),
    axisLabel: { color: '#667085', rotate: 28, width: 92, overflow: 'truncate' }
  },
  yAxis: {
    type: 'value',
    axisLabel: { color: '#667085', formatter: (value: number) => `$${value}` },
    splitLine: { lineStyle: { color: '#edf2f7' } }
  },
  series: [
    {
      name: t('cost'),
      type: 'bar',
      barMaxWidth: 22,
      data: statisticsSummary.value.top_models.map((item) =>
        Number(microUsdToUsd(item.cost_micro_usd).toFixed(6))
      )
    }
  ]
}))
async function reloadStatistics() {
  statisticsUsersPage.value = 1
  statisticsModelsPage.value = 1
  await Promise.all([
    reloadStatisticsSummary(),
    reloadStatisticsUsers(),
    reloadStatisticsModels(),
    reloadStatisticsOptions()
  ])
}

async function handleStatisticsUserPageChange(page: number) {
  statisticsUsersPage.value = page
  await reloadStatisticsUsers()
}

async function handleStatisticsModelPageChange(page: number) {
  statisticsModelsPage.value = page
  await reloadStatisticsModels()
}

async function handleStatisticsUserPageSizeChange(size: number) {
  statisticsUsersPageSize.value = size
  statisticsUsersPage.value = 1
  await reloadStatisticsUsers()
}

async function handleStatisticsModelPageSizeChange(size: number) {
  statisticsModelsPageSize.value = size
  statisticsModelsPage.value = 1
  await reloadStatisticsModels()
}

async function applyQuickRange(days: number) {
  statisticsFilters.dateRange = defaultStatisticsRange(days)
  await reloadStatistics()
}

async function handleProviderChange() {
  statisticsFilters.model = ''
  await reloadStatistics()
}

async function exportStatistics(scope: string | number | object) {
  if (typeof scope !== 'string') return
  statisticsExporting.value = true
  try {
    const result = await downloadAdminUsageStatisticsCsv(
      scope as UsageStatisticsExportScope,
      statisticsBaseQuery.value
    )
    downloadBlob(result.filename ?? `usage-statistics-${scope}.csv`, result.blob)
  } finally {
    statisticsExporting.value = false
  }
}

function defaultStatisticsRange(days: number) {
  const end = new Date()
  const start = new Date()
  start.setDate(end.getDate() - (days - 1))
  return [toDateKey(start), toDateKey(end)]
}

function emptyStatisticsSummary(): UsageStatisticsSummary {
  return {
    start: '',
    end: '',
    totals: {
      request_count: 0,
      success_count: 0,
      error_count: 0,
      streamed_count: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      cache_in_tokens: 0,
      cache_write_tokens: 0,
      reason_out_tokens: 0,
      audio_in_tokens: 0,
      audio_out_tokens: 0,
      billable_units: 0,
      cost_micro_usd: 0,
      avg_latency_ms: null,
      avg_first_response_ms: null
    },
    daily: [],
    top_users: [],
    top_models: [],
    providers: []
  }
}

function emptyStatisticsPage<T>(): UsageStatisticsPage<T> {
  return { items: [], total: 0, page: 1, limit: 20 }
}

function filledDailyRows(summary: UsageStatisticsSummary) {
  const [startValue, endValue] = statisticsFilters.dateRange ?? []
  if (!startValue || !endValue) return summary.daily
  const values = new Map(summary.daily.map((item) => [item.date, item]))
  const rows = []
  const current = new Date(`${startValue}T00:00:00`)
  const end = new Date(`${endValue}T00:00:00`)
  while (current <= end && rows.length <= 366) {
    const date = toDateKey(current)
    rows.push(
      values.get(date) ?? {
        date,
        request_count: 0,
        success_count: 0,
        error_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        billable_units: 0,
        cost_micro_usd: 0,
        avg_latency_ms: null
      }
    )
    current.setDate(current.getDate() + 1)
  }
  return rows
}

function modelDisplay(provider: string, model: string) {
  return model ? `${provider}/${model}` : provider
}

function billingMeterLabel(value?: string | null) {
  if (value === 'image') return t('billingMeterImageGeneration')
  if (value === 'token') return t('billingMeterToken')
  return t('billingMeterAll')
}

function modelStatisticsRowKey(row: ModelUsageStatistics) {
  return `${row.provider}/${row.model}/${row.billing_meter}`
}

function successRate(success: number, total: number) {
  if (total <= 0) return '-'
  return `${((success / total) * 100).toFixed(1)}%`
}

function trendTooltip(params: unknown, label: string, mode: 'cost' | 'number') {
  const items = Array.isArray(params) ? params : [params]
  const first = items[0] as { axisValue?: string } | undefined
  return [
    first?.axisValue ?? '',
    ...items.map((item) => {
      const current = item as { marker?: string; seriesName?: string; value?: number }
      const value =
        mode === 'cost'
          ? `$${Number(current.value ?? 0).toFixed(6)}`
          : formatNumber(Number(current.value ?? 0), locale.value)
      return `${current.marker ?? ''}${current.seriesName ?? label}: ${value}`
    })
  ].join('<br/>')
}

</script>

<template>
  <section class="grid usage-view usage-statistics-view">
    <div class="usage-statistics">
          <el-form class="usage-toolbar statistics-toolbar" @submit.prevent="reloadStatistics">
            <div class="usage-toolbar-filters">
              <label class="admin-filter-field">
                <span>{{ t('timeRange') }}</span>
                <el-date-picker
                  v-model="statisticsFilters.dateRange"
                  class="usage-date-range"
                  type="daterange"
                  value-format="YYYY-MM-DD"
                  :range-separator="t('to')"
                  :start-placeholder="t('startTime')"
                  :end-placeholder="t('endTime')"
                />
              </label>
              <label class="admin-filter-field">
                <span>{{ t('userSearch') }}</span>
                <el-input
                  v-model="statisticsFilters.userQuery"
                  class="usage-search-input"
                  clearable
                  :prefix-icon="Search"
                  :placeholder="t('userSearchPlaceholder')"
                />
              </label>
              <label class="admin-filter-field">
                <span>{{ t('provider') }}</span>
                <el-select
                  v-model="statisticsFilters.provider"
                  class="usage-status-filter"
                  clearable
                  filterable
                  :placeholder="t('allProviders')"
                  @change="handleProviderChange"
                >
                  <el-option
                    v-for="provider in statisticsOptions.providers"
                    :key="provider"
                    :label="provider"
                    :value="provider"
                  />
                </el-select>
              </label>
              <label class="admin-filter-field">
                <span>{{ t('model') }}</span>
                <el-select
                  v-model="statisticsFilters.model"
                  class="usage-model-filter"
                  clearable
                  filterable
                  :placeholder="t('allModels')"
                >
                  <el-option
                    v-for="item in filteredModelOptions"
                    :key="`${item.provider}/${item.model}`"
                    :label="modelDisplay(item.provider, item.model)"
                    :value="item.model"
                  />
                </el-select>
              </label>
              <label class="admin-filter-field">
                <span>{{ t('billingMeter') }}</span>
                <el-select v-model="statisticsFilters.billingMeter" class="usage-status-filter">
                  <el-option :label="t('allBillingMeters')" value="" />
                  <el-option :label="t('billingMeterToken')" value="token" />
                  <el-option :label="t('billingMeterImageGeneration')" value="image" />
                </el-select>
              </label>
              <el-button
                class="admin-action-button"
                type="primary"
                native-type="submit"
                :icon="Search"
                :loading="statisticsLoading"
              >
                {{ t('search') }}
              </el-button>
            </div>
            <div class="usage-toolbar-actions">
              <el-dropdown trigger="click" @command="exportStatistics">
                <el-button
                  class="admin-action-button"
                  :icon="Download"
                  :loading="statisticsExporting"
                  :disabled="statisticsEmpty"
                >
                  {{ t('exportStatistics') }}
                </el-button>
                <template #dropdown>
                  <el-dropdown-menu>
                    <el-dropdown-item command="users">{{ t('exportUserSummary') }}</el-dropdown-item>
                    <el-dropdown-item command="daily">{{ t('exportDailyTrend') }}</el-dropdown-item>
                    <el-dropdown-item command="models">{{ t('exportModelSummary') }}</el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
            </div>
            <div class="statistics-quick-ranges">
              <el-button @click="applyQuickRange(7)">{{ t('quickRange7') }}</el-button>
              <el-button @click="applyQuickRange(30)">{{ t('quickRange30') }}</el-button>
              <el-button @click="applyQuickRange(90)">{{ t('quickRange90') }}</el-button>
            </div>
          </el-form>

          <div class="statistics-metric-grid">
            <div class="statistics-metric">
              <span>{{ t('totalCost') }}</span>
              <strong>{{ formatMicroUsd(statisticsSummary.totals.cost_micro_usd, 6) }}</strong>
            </div>
            <div class="statistics-metric">
              <span>{{ t('requestCount') }}</span>
              <strong>{{ formatNumber(statisticsSummary.totals.request_count, locale) }}</strong>
            </div>
            <div class="statistics-metric">
              <span>{{ t('tokens') }}</span>
              <strong>{{ formatNumber(statisticsSummary.totals.total_tokens, locale) }}</strong>
            </div>
            <div class="statistics-metric">
              <span>{{ t('successRate') }}</span>
              <strong>{{
                successRate(
                  statisticsSummary.totals.success_count,
                  statisticsSummary.totals.request_count
                )
              }}</strong>
            </div>
            <div class="statistics-metric">
              <span>{{ t('averageLatency') }}</span>
              <strong>{{ formatDurationMs(statisticsSummary.totals.avg_latency_ms) }}</strong>
            </div>
          </div>

          <div
            v-if="statisticsInitialLoading"
            v-loading="true"
            class="statistics-loading-panel service-table-panel"
          ></div>

          <template v-else>
            <div class="statistics-chart-grid">
              <section class="statistics-panel">
                <header>{{ t('costTrend') }}</header>
                <AdminUsageChart
                  :option="costTrendOption"
                  :loading="statisticsSummaryLoading"
                  :empty="statisticsEmpty"
                  :empty-text="t('noStatisticsData')"
                  height="300px"
                />
              </section>
              <section class="statistics-panel">
                <header>{{ t('requestTrend') }}</header>
                <AdminUsageChart
                  :option="requestTrendOption"
                  :loading="statisticsSummaryLoading"
                  :empty="statisticsEmpty"
                  :empty-text="t('noStatisticsData')"
                  height="300px"
                />
              </section>
              <section class="statistics-panel">
                <header>{{ t('topUsersByCost') }}</header>
                <AdminUsageChart
                  :option="topUsersOption"
                  :loading="statisticsSummaryLoading"
                  :empty="statisticsSummary.top_users.length === 0"
                  :empty-text="t('noStatisticsData')"
                  height="320px"
                />
              </section>
              <section class="statistics-panel">
                <header>{{ t('topModelsByCost') }}</header>
                <AdminUsageChart
                  :option="topModelsOption"
                  :loading="statisticsSummaryLoading"
                  :empty="statisticsSummary.top_models.length === 0"
                  :empty-text="t('noStatisticsData')"
                  height="320px"
                />
              </section>
            </div>

            <section class="statistics-panel">
              <header>{{ t('userSummary') }}</header>
              <el-table
                v-loading="statisticsUsersLoading"
                class="admin-table service-table statistics-table"
                :data="statisticsUsers.items"
                row-key="user_id"
                stripe
              >
                <el-table-column :label="t('usageUser')" min-width="190">
                  <template #default="{ row }">
                    <div class="statistics-user-cell">
                      <strong>{{ row.user_display_name }}</strong>
                      <span v-if="row.user_id != null">#{{ row.user_id }}</span>
                    </div>
                  </template>
                </el-table-column>
                <el-table-column :label="t('requestCount')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
                </el-table-column>
                <el-table-column :label="t('successRate')" min-width="110" align="right">
                  <template #default="{ row }">{{
                    successRate(row.success_count, row.request_count)
                  }}</template>
                </el-table-column>
                <el-table-column :label="t('tokens')" min-width="130" align="right">
                  <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
                </el-table-column>
                <el-table-column :label="t('billingUnits')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatNumber(row.billable_units, locale) }}</template>
                </el-table-column>
                <el-table-column :label="t('cost')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatMicroUsd(row.cost_micro_usd, 6) }}</template>
                </el-table-column>
                <el-table-column :label="t('averageLatencyShort')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatDurationMs(row.avg_latency_ms) }}</template>
                </el-table-column>
                <el-table-column :label="t('modelCount')" min-width="100" align="right">
                  <template #default="{ row }">{{ formatNumber(row.model_count, locale) }}</template>
                </el-table-column>
                <template #empty>
                  <el-empty :description="t('noStatisticsData')" />
                </template>
              </el-table>
              <div class="statistics-pagination">
                <el-pagination
                  v-model:current-page="statisticsUsersPage"
                  v-model:page-size="statisticsUsersPageSize"
                  background
                  layout="total, sizes, prev, pager, next"
                  :total="statisticsUsers.total"
                  :page-sizes="[20, 50, 100]"
                  @current-change="handleStatisticsUserPageChange"
                  @size-change="handleStatisticsUserPageSizeChange"
                />
              </div>
            </section>

            <section class="statistics-panel">
              <header>{{ t('modelSummary') }}</header>
              <el-table
                v-loading="statisticsModelsLoading"
                class="admin-table service-table statistics-table"
                :data="statisticsModels.items"
                :row-key="modelStatisticsRowKey"
                stripe
              >
                <el-table-column :label="t('providerOrModel')" min-width="220">
                  <template #default="{ row }">
                    <div class="usage-model">
                      <span class="usage-provider">{{ row.provider }}</span>
                      <span>{{ row.model || '-' }}</span>
                    </div>
                  </template>
                </el-table-column>
                <el-table-column :label="t('billingMeter')" min-width="110">
                  <template #default="{ row }">{{ billingMeterLabel(row.billing_meter) }}</template>
                </el-table-column>
                <el-table-column :label="t('requestCount')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
                </el-table-column>
                <el-table-column :label="t('tokens')" min-width="130" align="right">
                  <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
                </el-table-column>
                <el-table-column :label="t('cost')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatMicroUsd(row.cost_micro_usd, 6) }}</template>
                </el-table-column>
                <el-table-column :label="t('averageLatencyShort')" min-width="120" align="right">
                  <template #default="{ row }">{{ formatDurationMs(row.avg_latency_ms) }}</template>
                </el-table-column>
                <el-table-column :label="t('userCount')" min-width="100" align="right">
                  <template #default="{ row }">{{ formatNumber(row.user_count, locale) }}</template>
                </el-table-column>
                <template #empty>
                  <el-empty :description="t('noStatisticsData')" />
                </template>
              </el-table>
              <div class="statistics-pagination">
                <el-pagination
                  v-model:current-page="statisticsModelsPage"
                  v-model:page-size="statisticsModelsPageSize"
                  background
                  layout="total, sizes, prev, pager, next"
                  :total="statisticsModels.total"
                  :page-sizes="[20, 50, 100]"
                  @current-change="handleStatisticsModelPageChange"
                  @size-change="handleStatisticsModelPageSizeChange"
                />
              </div>
            </section>
          </template>
        </div>
  </section>
</template>

<style scoped>
.statistics-toolbar {
  align-items: center;
  background: #ffffff;
  border: 1px solid #dfe8f2;
  border-radius: 8px;
  box-shadow: 0 10px 30px rgba(15, 23, 42, 0.04);
  display: grid;
  gap: 10px 12px;
  grid-template-columns: minmax(0, 1fr) auto;
  padding: 14px 16px;
}

.usage-statistics {
  display: grid;
  gap: 18px;
  min-width: 0;
}

.statistics-quick-ranges {
  display: flex;
  gap: 4px;
  grid-column: 1 / -1;
  padding-top: 2px;
}

.statistics-quick-ranges .el-button {
  --el-button-bg-color: transparent;
  --el-button-border-color: transparent;
  --el-button-hover-bg-color: #f2f6fb;
  --el-button-hover-border-color: transparent;
  --el-button-hover-text-color: #475467;
  --el-button-text-color: #667085;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 620;
  height: 30px;
  min-height: 30px;
  padding: 0 8px;
}

.statistics-toolbar .usage-toolbar-filters,
.statistics-toolbar .usage-toolbar-actions {
  row-gap: 10px;
}

.statistics-toolbar .usage-toolbar-filters {
  flex-wrap: nowrap;
}

.statistics-toolbar .usage-toolbar-actions {
  align-self: start;
  flex-wrap: nowrap;
  justify-content: flex-end;
}

.usage-model-filter {
  width: 180px;
}

.statistics-toolbar .admin-filter-field {
  flex: 0 1 auto;
  gap: 6px;
}

.statistics-toolbar .admin-filter-field > span {
  color: #667085;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 680;
  white-space: nowrap;
}

.statistics-toolbar .usage-date-range.el-date-editor.el-input__wrapper {
  flex-basis: 250px;
  width: 250px;
}

.statistics-toolbar .usage-search-input.el-input {
  flex-basis: 190px;
  width: 190px;
}

.statistics-toolbar .usage-status-filter.el-select {
  flex-basis: 118px;
  width: 118px;
}

.statistics-metric-grid {
  display: grid;
  gap: 16px;
  grid-template-columns: repeat(5, minmax(150px, 1fr));
}

.statistics-metric,
.statistics-panel {
  background: #ffffff;
  border: 1px solid #dfe8f2;
  border-radius: 8px;
  box-shadow: 0 10px 30px rgba(15, 23, 42, 0.04);
}

.statistics-metric {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 94px;
  padding: 16px;
}

.statistics-metric span {
  color: #667085;
  font-size: 13px;
  font-weight: 600;
}

.statistics-metric strong {
  color: #101828;
  font-feature-settings: 'tnum';
  font-size: 22px;
  font-variant-numeric: tabular-nums;
  font-weight: 760;
}

.statistics-loading-panel {
  min-height: 420px;
}

.statistics-chart-grid {
  display: grid;
  gap: 18px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.statistics-panel {
  min-width: 0;
  padding: 18px;
}

.statistics-panel.is-wide {
  grid-column: 1 / -1;
}

.statistics-panel header {
  color: #1d2939;
  font-size: 15px;
  font-weight: 720;
  margin-bottom: 12px;
}

.statistics-table {
  width: 100%;
}

.statistics-user-cell {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.statistics-user-cell strong {
  color: #1d2939;
  font-size: 13px;
  font-weight: 680;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.statistics-user-cell span {
  color: #98a2b3;
  font-size: 12px;
  font-weight: 560;
}

.statistics-pagination {
  display: flex;
  justify-content: flex-end;
  padding-top: 16px;
}


@media (max-width: 1180px) {
  .statistics-metric-grid,
  .statistics-chart-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .statistics-toolbar {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .statistics-toolbar .usage-toolbar-filters {
    flex-wrap: wrap;
  }

  .statistics-toolbar .usage-toolbar-actions {
    justify-content: flex-start;
  }
}

@media (max-width: 760px) {
  .usage-statistics {
    gap: 14px;
  }

  .statistics-toolbar,
  .statistics-panel {
    padding: 14px;
  }

  .statistics-toolbar .usage-toolbar-filters,
  .statistics-toolbar .usage-toolbar-actions {
    display: grid;
    grid-template-columns: 1fr;
  }

  .statistics-toolbar .admin-filter-field {
    align-items: stretch;
    display: grid;
    gap: 5px;
  }

  .statistics-metric-grid,
  .statistics-chart-grid {
    grid-template-columns: 1fr;
  }

  .statistics-metric-grid,
  .statistics-chart-grid {
    gap: 14px;
  }

  .statistics-panel.is-wide {
    grid-column: auto;
  }

  .statistics-quick-ranges {
    width: 100%;
  }

  .statistics-quick-ranges .el-button {
    flex: 1 1 0;
    height: 32px;
    min-width: 0;
  }

  .statistics-pagination {
    justify-content: flex-start;
    overflow-x: auto;
  }
}
</style>
