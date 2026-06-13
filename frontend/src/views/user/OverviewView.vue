<script setup lang="ts">
import { computed, inject, ref, type Ref } from 'vue'
import { Calendar, DataLine, Document, Key, Monitor, Wallet } from '@element-plus/icons-vue'
import { RouterLink } from 'vue-router'
import { getUserOverview } from '../../api/overview'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import { formatMicroUsd, toDateKey } from '../../utils/format'
import type { ServicePolicy } from '../../api/policy'

const { t } = useLocale()
const {
  data: overview,
  loading,
  loaded: overviewLoaded
} = useAsyncData(() => getUserOverview(), null)
const servicePolicy = inject<Ref<ServicePolicy | null>>('servicePolicy')!
const hoveredChartIndex = ref<number | null>(null)
const overviewInitialLoading = computed(() => !overviewLoaded.value)
const showBalance = computed(() =>
  Boolean(servicePolicy.value?.credit_required || servicePolicy.value?.recharge_enabled)
)
const showRecharge = computed(() => Boolean(servicePolicy.value?.recharge_enabled))

const usageMetricCards = computed(() => [
  {
    key: 'todayCost',
    label: t('todayCost'),
    value: formatMicroUsd(overview.value?.today_cost_micro_usd),
    trend: buildTrend(
      todayCost.value,
      yesterdayCost.value,
      t('comparedWithYesterday'),
      t('trendNoYesterdayCost')
    ),
    icon: DataLine
  },
  {
    key: 'monthCost',
    label: t('monthCost'),
    value: formatMicroUsd(overview.value?.month_cost_micro_usd),
    trend: buildTrend(
      currentMonthCost.value,
      previousMonthSamePeriodCost.value,
      t('comparedWithLastMonth'),
      t('trendNoBaseline')
    ),
    icon: Calendar
  }
])

const quickActions = computed(() => [
  {
    key: 'apiKeys',
    label: t('manageApiKeys'),
    hint: t('apiKeysHint'),
    to: '/home/apikeys',
    icon: Key
  },
  {
    key: 'usage',
    label: t('viewUsageDetails'),
    hint: t('usageHint'),
    to: '/home/usage',
    icon: Monitor
  },
  {
    key: 'docs',
    label: t('viewHelpDocs'),
    hint: t('helpDocsHint'),
    to: '/docs',
    icon: Document,
    newPage: true
  }
])

const dailyCostMap = computed(() => {
  const map = new Map<string, number>()
  for (const item of overview.value?.daily_costs ?? []) {
    map.set(item.date, item.cost_micro_usd)
  }
  return map
})

const chartPoints = computed(() => {
  const days = Array.from({ length: 30 }, (_, index) => {
    const date = new Date()
    date.setHours(0, 0, 0, 0)
    date.setDate(date.getDate() - (29 - index))
    const key = toDateKey(date)
    return {
      date: key,
      label: formatChartDate(date),
      cost_micro_usd: dailyCostMap.value.get(key) ?? 0
    }
  })
  const maxCost = Math.max(...days.map((item) => item.cost_micro_usd), 1)

  return days.map((item, index) => ({
    ...item,
    x: (index / 29) * 100,
    y: 88 - (item.cost_micro_usd / maxCost) * 72
  }))
})

const chartPolyline = computed(() =>
  chartPoints.value.map((point) => `${point.x},${point.y}`).join(' ')
)
const chartArea = computed(() => `0,92 ${chartPolyline.value} 100,92`)
const chartAxisTicks = computed(() => {
  const indexes = [0, 7, 14, 21, 29]
  return indexes
    .map((index) => chartPoints.value[index])
    .filter((point): point is NonNullable<typeof point> => Boolean(point))
})
const hoveredChartPoint = computed(() =>
  hoveredChartIndex.value == null ? null : (chartPoints.value[hoveredChartIndex.value] ?? null)
)
const chartTooltipStyle = computed(() => {
  const point = hoveredChartPoint.value
  if (!point) return {}
  const x = Math.min(Math.max(point.x, 12), 88)
  return {
    left: `${x}%`,
    top: `${point.y}%`
  }
})
const balanceEstimate = computed(() => {
  const available = overview.value?.available_micro_usd ?? 0
  const recentCosts = chartPoints.value.slice(-7).map((item) => item.cost_micro_usd)
  const averageDailyCost = recentCosts.reduce((sum, cost) => sum + cost, 0) / recentCosts.length

  if (available <= 0 || averageDailyCost <= 0) return t('balanceEstimateUnavailable')

  const days = Math.floor(available / averageDailyCost)
  if (days >= 365) return t('balanceEstimateLong')
  return t('balanceEstimateDays').replace('{days}', Math.max(days, 1).toLocaleString())
})
const todayCost = computed(() => getCostForDate(new Date()))
const yesterdayCost = computed(() => {
  const date = new Date()
  date.setDate(date.getDate() - 1)
  return getCostForDate(date)
})
const currentMonthCost = computed(() => overview.value?.month_cost_micro_usd ?? 0)
const previousMonthSamePeriodCost = computed(() => {
  const today = new Date()
  const dayCount = today.getDate()
  let total = 0

  for (let index = 1; index <= dayCount; index += 1) {
    const date = new Date(today.getFullYear(), today.getMonth() - 1, index)
    total += getCostForDate(date)
  }

  return total
})

function formatPercent(value: number) {
  const sign = value > 0 ? '+' : ''
  return `${sign}${value.toFixed(0)}%`
}

function buildTrend(current: number, baseline: number, label: string, emptyLabel: string) {
  if (baseline <= 0) {
    return {
      label: current > 0 ? emptyLabel : t('trendNoChange'),
      value: '',
      tone: 'neutral'
    }
  }

  const percent = ((current - baseline) / baseline) * 100
  return {
    label,
    value: formatPercent(percent),
    tone: percent > 0 ? 'up' : percent < 0 ? 'down' : 'neutral'
  }
}

function formatChartDate(date: Date) {
  return `${date.getMonth() + 1}/${date.getDate()}`
}

function formatFullChartDate(value: string) {
  const date = new Date(`${value}T00:00:00`)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

function getCostForDate(date: Date) {
  return dailyCostMap.value.get(toDateKey(date)) ?? 0
}
</script>

<template>
  <section class="user-overview-view">
    <div
      v-if="overviewInitialLoading"
      class="overview-summary-grid overview-loading-grid"
      aria-hidden="true"
    >
      <div class="user-panel overview-card-skeleton overview-balance-card-skeleton">
        <span></span>
        <strong></strong>
        <small></small>
      </div>
      <div class="user-panel overview-card-skeleton">
        <span></span>
        <strong></strong>
        <small></small>
      </div>
      <div class="user-panel overview-card-skeleton">
        <span></span>
        <strong></strong>
        <small></small>
      </div>
    </div>
    <div
      v-else
      v-loading="loading"
      class="overview-summary-grid"
      :class="{ 'without-balance': !showBalance }"
    >
      <div v-if="showBalance" class="user-panel overview-balance-card">
        <div class="overview-balance-watermark" aria-hidden="true">
          <el-icon><Wallet /></el-icon>
        </div>
        <div class="overview-balance-heading">
          <div class="overview-dashboard-icon">
            <el-icon><Wallet /></el-icon>
          </div>
          <span>{{ t('currentBalance') }}</span>
        </div>
        <div class="overview-balance-value">
          <strong>{{ formatMicroUsd(overview?.available_micro_usd) }}</strong>
          <small class="overview-balance-estimate">{{ balanceEstimate }}</small>
        </div>
        <el-button
          v-if="showRecharge"
          class="overview-card-cta"
          :tag="RouterLink"
          to="/home/recharge"
        >
          {{ t('goRecharge') }}
        </el-button>
      </div>

      <div v-for="item in usageMetricCards" :key="item.key" class="user-panel overview-usage-card">
        <div class="overview-dashboard-icon">
          <el-icon><component :is="item.icon" /></el-icon>
        </div>
        <span>{{ item.label }}</span>
        <strong>{{ item.value }}</strong>
        <small class="overview-usage-trend" :class="item.trend.tone">
          <b v-if="item.trend.value">{{ item.trend.value }}</b>
          <span>{{ item.trend.label }}</span>
        </small>
      </div>
    </div>

    <div
      v-if="overviewInitialLoading"
      class="user-panel overview-trend-panel overview-trend-skeleton"
    >
      <div class="overview-trend-skeleton-header">
        <span></span>
        <strong></strong>
      </div>
      <div class="overview-trend-skeleton-chart">
        <i></i>
        <i></i>
        <i></i>
      </div>
    </div>
    <div v-else v-loading="loading" class="user-panel overview-trend-panel">
      <div class="user-section-header">
        <div>
          <span class="user-eyebrow">{{ t('trendSummary') }}</span>
          <h3>{{ formatMicroUsd(overview?.month_cost_micro_usd) }}</h3>
        </div>
        <span>{{ t('trendPill') }}</span>
      </div>
      <div class="overview-chart-wrap" @mouseleave="hoveredChartIndex = null">
        <svg
          class="overview-line-chart"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          role="img"
          :aria-label="t('last30DaysCost')"
        >
          <polygon class="overview-chart-area" :points="chartArea" />
          <polyline class="overview-chart-line" :points="chartPolyline" />
          <g v-if="hoveredChartPoint" class="overview-chart-active">
            <line :x1="hoveredChartPoint.x" y1="8" :x2="hoveredChartPoint.x" y2="92" />
          </g>
          <rect
            v-for="(point, index) in chartPoints"
            :key="point.date"
            class="overview-chart-hit-area"
            :x="Math.max(point.x - 1.8, 0)"
            y="0"
            width="3.6"
            height="100"
            tabindex="0"
            role="button"
            :aria-label="`${formatFullChartDate(point.date)} ${formatMicroUsd(point.cost_micro_usd)}`"
            @mouseenter="hoveredChartIndex = index"
            @mousemove="hoveredChartIndex = index"
            @focus="hoveredChartIndex = index"
            @blur="hoveredChartIndex = null"
          />
        </svg>
        <div v-if="hoveredChartPoint" class="overview-chart-tooltip" :style="chartTooltipStyle">
          <span>{{ formatFullChartDate(hoveredChartPoint.date) }}</span>
          <strong>{{ formatMicroUsd(hoveredChartPoint.cost_micro_usd) }}</strong>
        </div>
      </div>
      <div class="overview-chart-axis">
        <span v-for="tick in chartAxisTicks" :key="tick.date" :style="{ left: `${tick.x}%` }">
          {{ tick.label }}
        </span>
      </div>
    </div>

    <div class="overview-action-grid">
      <template v-for="item in quickActions" :key="item.key">
        <a
          v-if="item.newPage"
          class="user-panel overview-action-card"
          :href="item.to"
          target="_blank"
          rel="noopener noreferrer"
        >
          <span class="overview-action-icon">
            <el-icon><component :is="item.icon" /></el-icon>
          </span>
          <span class="overview-action-copy">
            <strong>{{ item.label }}</strong>
            <span>{{ item.hint }}</span>
          </span>
        </a>
        <RouterLink v-else class="user-panel overview-action-card" :to="item.to">
          <span class="overview-action-icon">
            <el-icon><component :is="item.icon" /></el-icon>
          </span>
          <span class="overview-action-copy">
            <strong>{{ item.label }}</strong>
            <span>{{ item.hint }}</span>
          </span>
        </RouterLink>
      </template>
    </div>
  </section>
</template>

<style scoped>
.user-overview-view {
  display: grid;
  gap: 12px;
  width: min(1120px, 100%);
}

.overview-summary-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1.18fr) minmax(280px, 0.82fr);
  grid-template-rows: repeat(2, minmax(132px, 1fr));
}

.overview-summary-grid.without-balance {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-template-rows: none;
}

.overview-loading-grid {
  min-height: 276px;
}

.overview-card-skeleton {
  align-content: center;
  display: grid;
  gap: 16px;
  min-height: 126px;
  padding: 24px 22px;
}

.overview-balance-card-skeleton {
  grid-row: 1 / span 2;
  min-height: 276px;
}

.overview-card-skeleton span,
.overview-card-skeleton strong,
.overview-card-skeleton small,
.overview-trend-skeleton span,
.overview-trend-skeleton strong,
.overview-trend-skeleton i {
  background: var(--skeleton-gradient);
  background-size: 220% 100%;
  border-radius: 999px;
  display: block;
}

.overview-card-skeleton span {
  height: 12px;
  width: 38%;
}

.overview-card-skeleton strong {
  height: 30px;
  width: 58%;
}

.overview-card-skeleton small {
  height: 12px;
  width: 72%;
}

.overview-balance-card,
.overview-usage-card {
  align-content: center;
  display: grid;
  gap: 13px;
  padding: 24px 22px;
  position: relative;
  transition:
    border-color 0.16s ease,
    box-shadow 0.16s ease;
}

.overview-balance-card:hover,
.overview-usage-card:hover,
.overview-trend-panel:hover,
.overview-action-card:hover {
  border-color: var(--user-primary-border, #b7dcf2);
  box-shadow:
    0 1px 2px rgba(15, 23, 42, 0.03),
    0 14px 34px rgba(15, 23, 42, 0.055);
}

.overview-balance-card {
  background:
    radial-gradient(circle at 88% 0%, rgba(255, 255, 255, 0.24), transparent 28%),
    linear-gradient(135deg, #0f76b8, #168bd3 56%, #69c2ed);
  border-color: transparent;
  color: #fff;
  display: grid;
  grid-row: 1 / span 2;
  grid-template-rows: auto 1fr auto;
  min-height: 276px;
  overflow: hidden;
  padding: 22px;
}

.overview-balance-card:hover {
  border-color: transparent;
}

.overview-usage-card {
  align-items: center;
  grid-template-columns: minmax(0, 1fr) 38px;
  min-height: 126px;
}

.overview-usage-card .overview-dashboard-icon {
  grid-column: 2;
  grid-row: 1 / span 3;
}

.overview-usage-card span,
.overview-usage-card strong,
.overview-usage-trend {
  grid-column: 1;
}

.overview-balance-card span,
.overview-usage-card span {
  color: #697586;
  font-size: 14px;
  font-weight: 760;
  line-height: 1.2;
}

.overview-balance-heading {
  align-items: center;
  display: flex;
  gap: 10px;
  position: relative;
  z-index: 1;
}

.overview-balance-card strong,
.overview-usage-card strong {
  color: #111827;
  font-size: 29px;
  font-weight: 840;
  line-height: 1.05;
}

.overview-balance-value {
  align-self: center;
  display: grid;
  gap: 10px;
  justify-items: start;
  position: relative;
  z-index: 1;
}

.overview-balance-card strong {
  align-self: center;
  font-size: 44px;
  letter-spacing: 0;
}

.overview-balance-estimate {
  background: rgba(255, 255, 255, 0.14);
  border: 1px solid rgba(255, 255, 255, 0.18);
  border-radius: 999px;
  color: #e4edf5;
  font-size: 12.5px;
  font-weight: 760;
  line-height: 1.2;
  padding: 6px 10px;
}

.overview-usage-trend {
  align-items: center;
  color: #7a8798;
  display: flex;
  flex-wrap: wrap;
  font-size: 12px;
  font-weight: 680;
  gap: 6px;
  line-height: 1.2;
}

.overview-usage-trend b {
  border-radius: 999px;
  font-size: 12px;
  font-weight: 820;
  padding: 3px 7px;
}

.overview-usage-trend.up b {
  background: #ecfdf3;
  color: #047857;
}

.overview-usage-trend.down b {
  background: #fef2f2;
  color: #b42318;
}

.overview-usage-trend.neutral b {
  background: #f3f6fb;
  color: #5f6b7a;
}

.overview-balance-card span,
.overview-balance-card strong {
  color: #fff;
}

.overview-dashboard-icon {
  align-items: center;
  background: var(--user-primary-soft, #eef4f9);
  border-radius: 8px;
  color: var(--user-primary, #168bd3);
  display: inline-flex;
  height: 36px;
  justify-content: center;
  width: 36px;
  box-shadow: inset 0 0 0 1px rgba(22, 139, 211, 0.1);
}

.overview-balance-card .overview-dashboard-icon {
  background: rgba(255, 255, 255, 0.18);
  color: #fff;
}

.overview-balance-watermark {
  bottom: 12px;
  color: rgba(255, 255, 255, 0.1);
  font-size: 118px;
  line-height: 1;
  position: absolute;
  right: 18px;
  transform: rotate(-8deg);
}

.overview-card-cta {
  --el-button-bg-color: #ffffff;
  --el-button-border-color: #ffffff;
  --el-button-hover-bg-color: var(--user-primary-soft, #eef4f9);
  --el-button-hover-border-color: #ffffff;
  --el-button-hover-text-color: var(--user-primary, #168bd3);
  border-radius: 7px;
  color: var(--user-primary, #168bd3);
  height: 34px;
  font-weight: 780;
  justify-self: start;
  min-width: 92px;
  padding: 0 16px;
  position: relative;
  text-decoration: none;
  z-index: 1;
}

.overview-trend-panel {
  display: grid;
  gap: 12px;
  min-height: 236px;
  padding: 22px 22px 18px;
  transition:
    border-color 0.16s ease,
    box-shadow 0.16s ease;
}

.overview-trend-skeleton {
  align-content: stretch;
}

.overview-trend-skeleton-header {
  display: grid;
  gap: 10px;
}

.overview-trend-skeleton-header span {
  height: 11px;
  width: 116px;
}

.overview-trend-skeleton-header strong {
  height: 28px;
  width: 180px;
}

.overview-trend-skeleton-chart {
  align-items: end;
  background:
    linear-gradient(
      to bottom,
      transparent 0 24%,
      rgba(226, 232, 240, 0.55) 24% 24.5%,
      transparent 24.5% 49%,
      rgba(226, 232, 240, 0.55) 49% 49.5%,
      transparent 49.5% 74%,
      rgba(226, 232, 240, 0.55) 74% 74.5%,
      transparent 74.5%
    ),
    #ffffff;
  border-radius: 8px;
  display: grid;
  gap: 14px;
  grid-template-columns: 1fr 1.5fr 0.8fr;
  min-height: 144px;
  padding: 20px;
}

.overview-trend-skeleton-chart i {
  height: 12px;
}

.overview-trend-skeleton-chart i:nth-child(1) {
  margin-bottom: 34px;
}

.overview-trend-skeleton-chart i:nth-child(2) {
  margin-bottom: 72px;
}

.overview-trend-skeleton-chart i:nth-child(3) {
  margin-bottom: 48px;
}

.overview-chart-wrap {
  height: 144px;
  position: relative;
}

.overview-line-chart {
  background:
    linear-gradient(
      to bottom,
      transparent 0 24%,
      rgba(226, 232, 240, 0.55) 24% 24.5%,
      transparent 24.5% 49%,
      rgba(226, 232, 240, 0.55) 49% 49.5%,
      transparent 49.5% 74%,
      rgba(226, 232, 240, 0.55) 74% 74.5%,
      transparent 74.5%
    ),
    linear-gradient(
      to right,
      transparent 0 24%,
      rgba(226, 232, 240, 0.32) 24% 24.3%,
      transparent 24.3% 49%,
      rgba(226, 232, 240, 0.32) 49% 49.3%,
      transparent 49.3% 74%,
      rgba(226, 232, 240, 0.32) 74% 74.3%,
      transparent 74.3%
    );
  border-radius: 8px;
  height: 100%;
  overflow: visible;
  width: 100%;
}

.overview-chart-area {
  fill: rgba(22, 139, 211, 0.13);
}

.overview-chart-line {
  fill: none;
  stroke: var(--user-primary, #168bd3);
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 2.5;
  vector-effect: non-scaling-stroke;
}

.overview-chart-active line {
  stroke: rgba(22, 139, 211, 0.38);
  stroke-dasharray: 3 3;
  stroke-width: 1;
  vector-effect: non-scaling-stroke;
}

.overview-chart-hit-area {
  cursor: crosshair;
  fill: transparent;
  outline: none;
  pointer-events: all;
}

.overview-chart-hit-area:focus-visible {
  fill: rgba(22, 139, 211, 0.1);
}

.overview-chart-tooltip {
  background: #111827;
  border-radius: 8px;
  box-shadow: 0 12px 28px rgba(15, 23, 42, 0.18);
  color: #ffffff;
  display: grid;
  gap: 4px;
  min-width: 116px;
  padding: 8px 10px;
  pointer-events: none;
  position: absolute;
  transform: translate(-50%, calc(-100% - 10px));
  z-index: 2;
}

.overview-chart-tooltip::after {
  border-left: 6px solid transparent;
  border-right: 6px solid transparent;
  border-top: 6px solid #111827;
  bottom: -5px;
  content: '';
  left: 50%;
  position: absolute;
  transform: translateX(-50%);
}

.overview-chart-tooltip span {
  color: #cbd5e1;
  font-size: 11px;
  font-weight: 680;
}

.overview-chart-tooltip strong {
  color: #ffffff;
  font-size: 14px;
  font-weight: 820;
}

.overview-chart-axis {
  color: #8a95a5;
  font-size: 12px;
  font-weight: 650;
  height: 14px;
  position: relative;
}

.overview-chart-axis span {
  position: absolute;
  top: 0;
  transform: translateX(-50%);
  white-space: nowrap;
}

.overview-chart-axis span:first-child {
  transform: translateX(0);
}

.overview-chart-axis span:last-child {
  transform: translateX(-100%);
}

.overview-action-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.overview-action-card {
  align-items: center;
  color: inherit;
  display: grid;
  gap: 12px;
  grid-template-columns: 40px minmax(0, 1fr);
  min-height: 98px;
  padding: 22px 20px;
  text-decoration: none;
  transition:
    border-color 0.16s ease,
    box-shadow 0.16s ease;
}

.overview-action-card:hover {
  border-color: var(--user-primary-border-strong, #82c4e8);
}

.overview-action-icon {
  align-items: center;
  background: #f4f7fb;
  border-radius: 8px;
  color: var(--user-primary, #168bd3);
  display: inline-flex;
  height: 36px;
  justify-content: center;
  width: 36px;
}

.overview-action-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.overview-action-copy strong {
  color: #111827;
  font-size: 15.5px;
  font-weight: 780;
}

.overview-action-copy span {
  color: #697586;
  font-size: 13px;
  line-height: 1.35;
}

@media (max-width: 640px) {
  .overview-summary-grid,
  .overview-action-grid {
    grid-template-columns: 1fr;
  }

  .overview-summary-grid {
    grid-template-rows: none;
  }

  .overview-balance-card {
    grid-row: auto;
    min-height: 210px;
    padding: 22px;
  }

  .overview-balance-card-skeleton {
    grid-row: auto;
    min-height: 210px;
  }

  .overview-usage-card {
    min-height: 132px;
  }

  .overview-balance-card strong,
  .overview-usage-card strong {
    font-size: 30px;
  }

  .overview-balance-card strong {
    font-size: 38px;
  }

  .overview-action-card {
    min-height: 88px;
    padding: 18px;
  }
}
</style>
