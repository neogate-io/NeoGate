<script setup lang="ts">
import { computed } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import type { Channel, ChannelProbeSample } from '../../../types/admin'
import { formatCompactDateTime, formatDurationMs } from '../../../utils/format'

const props = defineProps<{
  channel: Channel
  latencyScale: number
}>()

const { t } = useLocale()

const latestSample = computed(() =>
  props.channel.probe_samples.length > 0
    ? props.channel.probe_samples[props.channel.probe_samples.length - 1]
    : null
)

const trendStats = computed(() => {
  const samples = props.channel.probe_samples
  const latencySamples = samples.filter((sample) => sample.latency_ms != null)
  const latencyValues = latencySamples.map((sample) => sample.latency_ms ?? 0)
  const okCount = samples.filter((sample) => sample.status === 'ok').length
  const failedCount = samples.filter((sample) => sample.status === 'failed').length
  const avgLatency =
    latencyValues.length > 0
      ? latencyValues.reduce((sum, value) => sum + value, 0) / latencyValues.length
      : null
  const minLatency = latencyValues.length > 0 ? Math.min(...latencyValues) : null
  const maxLatency = latencyValues.length > 0 ? Math.max(...latencyValues) : null

  return {
    total: samples.length,
    okCount,
    failedCount,
    avgLatency,
    minLatency,
    maxLatency
  }
})

const trendBars = computed(() => {
  const samples = props.channel.probe_samples
  if (samples.length === 0) return []

  const width = 132
  const baseline = 39
  const maxHeight = 34
  const gap = samples.length > 18 ? 2 : 3
  const barWidth = Math.max(3, Math.min(10, (width - gap * (samples.length - 1)) / samples.length))
  const totalWidth = barWidth * samples.length + gap * (samples.length - 1)
  const offsetX = Math.max(0, (width - totalWidth) / 2)
  const maxLatency = props.latencyScale

  return samples.map((sample, index) => {
    const latency = sample.latency_ms ?? 0
    const valueHeight =
      sample.status === 'ok' && latency > 0
        ? Math.max(4, (latency / maxLatency) * maxHeight)
        : sample.status === 'failed'
          ? 16
          : 8
    const height = Math.min(maxHeight, valueHeight)

    return {
      key: `${sample.created_at}-${index}`,
      x: Number((offsetX + index * (barWidth + gap)).toFixed(2)),
      y: Number((baseline - height).toFixed(2)),
      width: Number(barWidth.toFixed(2)),
      height: Number(height.toFixed(2)),
      tone: probeSampleTone(sample),
      title: probeSampleTitle(sample)
    }
  })
})

const trendClass = computed(() => {
  const latest = latestSample.value
  if (!latest) return 'is-empty'
  return latest.status === 'ok' ? 'is-ok' : 'is-failed'
})

const successLabel = computed(() =>
  trendStats.value.total === 0 ? '-' : `${trendStats.value.okCount}/${trendStats.value.total}`
)

const averageLabel = computed(() =>
  trendStats.value.avgLatency == null ? '-' : formatDurationMs(trendStats.value.avgLatency)
)

const tooltip = computed(() => {
  const sample = latestSample.value
  if (!sample) return t('probeNoDataHint')
  const status = sample.status === 'ok' ? t('diagnosticStatusOk') : t('diagnosticStatusFailed')
  return [
    `${t('time')}: ${formatCompactDateTime(sample.created_at)}`,
    `${t('model')}: ${sample.model || '-'}`,
    `${t('channelStatus')}: ${status}`,
    `${t('latency')}: ${probeLatencyLabel(sample)}`,
    `${t('probeSuccessRatio')}: ${trendStats.value.okCount}/${trendStats.value.total}`,
    trendStats.value.avgLatency != null
      ? `${t('probeAverageLatency')}: ${formatDurationMs(trendStats.value.avgLatency)}`
      : '',
    trendStats.value.minLatency != null && trendStats.value.maxLatency != null
      ? `${t('probeLatencyRange')}: ${formatDurationMs(trendStats.value.minLatency)} - ${formatDurationMs(trendStats.value.maxLatency)}`
      : '',
    sample.status_code ? `${t('probeStatusCode')}: ${sample.status_code}` : '',
    sample.error_summary ? `${t('error')}: ${sample.error_summary}` : ''
  ]
    .filter(Boolean)
    .join('\n')
})

function probeSampleTone(sample: ChannelProbeSample) {
  if (sample.status === 'skipped') return 'is-skipped'
  if (sample.status !== 'ok') return 'is-failed'
  if (sample.latency_ms == null) return 'is-empty'
  if (sample.latency_ms > 5000) return 'is-very-slow'
  if (sample.latency_ms > 2000) return 'is-slow'
  return 'is-ok'
}

function probeSampleStatusText(sample: ChannelProbeSample) {
  if (sample.status === 'ok') return t('diagnosticStatusOk')
  if (sample.status === 'skipped') return t('diagnosticStatusSkipped')
  return t('diagnosticStatusFailed')
}

function probeLatencyLabel(sample: ChannelProbeSample | null) {
  if (!sample) return t('probeNoData')
  if (sample.status !== 'ok') return t('probeFailed')
  return sample.latency_ms == null ? '-' : `${sample.latency_ms}ms`
}

function probeSampleStatusLabel(sample: ChannelProbeSample | null) {
  if (!sample) return t('probeNoData')
  if (sample.status === 'ok') return t('diagnosticStatusOk')
  if (sample.status === 'skipped') return t('diagnosticStatusSkipped')
  return t('diagnosticStatusFailed')
}

function probeSampleTitle(sample: ChannelProbeSample) {
  return [
    `${t('time')}: ${formatCompactDateTime(sample.created_at)}`,
    `${t('model')}: ${sample.model || '-'}`,
    `${t('channelStatus')}: ${probeSampleStatusText(sample)}`,
    `${t('latency')}: ${probeLatencyLabel(sample)}`,
    sample.status_code ? `${t('probeStatusCode')}: ${sample.status_code}` : '',
    sample.error_summary ? `${t('error')}: ${sample.error_summary}` : ''
  ]
    .filter(Boolean)
    .join('\n')
}
</script>

<template>
  <el-tooltip
    placement="top"
    effect="light"
    popper-class="probe-trend-tooltip"
    :show-after="600"
  >
    <template #content>
      <div class="probe-tooltip-content">
        <div class="probe-tooltip-head">
          <div class="probe-tooltip-title">{{ t('probeLatestResult') }}</div>
          <div class="probe-tooltip-subtitle">
            {{ latestSample?.model || '-' }}
          </div>
        </div>
        <div class="probe-tooltip-grid">
          <span>{{ t('time') }}</span>
          <strong>
            {{ latestSample ? formatCompactDateTime(latestSample.created_at) : '-' }}
          </strong>
          <span>{{ t('channelStatus') }}</span>
          <strong>{{ probeSampleStatusLabel(latestSample) }}</strong>
          <span>{{ t('latency') }}</span>
          <strong>{{ probeLatencyLabel(latestSample) }}</strong>
          <span>{{ t('probeSuccessRatio') }}</span>
          <strong>{{ successLabel }}</strong>
          <span>{{ t('probeAverageLatency') }}</span>
          <strong>{{ averageLabel }}</strong>
          <template v-if="trendStats.minLatency != null && trendStats.maxLatency != null">
            <span>{{ t('probeLatencyRange') }}</span>
            <strong>
              {{ formatDurationMs(trendStats.minLatency) }} -
              {{ formatDurationMs(trendStats.maxLatency) }}
            </strong>
          </template>
          <template v-if="latestSample?.status_code">
            <span>{{ t('probeStatusCode') }}</span>
            <strong>{{ latestSample.status_code }}</strong>
          </template>
          <template v-if="latestSample?.error_summary">
            <span>{{ t('error') }}</span>
            <strong>{{ latestSample.error_summary }}</strong>
          </template>
        </div>
      </div>
    </template>
    <div class="probe-trend-cell" :class="trendClass" :aria-label="tooltip">
      <div v-if="trendBars.length === 0" class="probe-trend-empty" aria-hidden="true">
        <span>{{ t('probeNoData') }}</span>
      </div>
      <svg v-else class="probe-trend-chart" viewBox="0 0 132 44" aria-hidden="true">
        <line
          x1="0"
          y1="39"
          x2="132"
          y2="39"
          class="probe-trend-baseline"
          stroke-linecap="round"
        />
        <rect
          v-for="bar in trendBars"
          :key="bar.key"
          :x="bar.x"
          :y="bar.y"
          :width="bar.width"
          :height="bar.height"
          rx="1.8"
          class="probe-trend-bar"
          :class="bar.tone"
        >
          <title>{{ bar.title }}</title>
        </rect>
      </svg>
      <div v-if="trendBars.length > 0" class="probe-trend-foot">
        <span>{{ t('probeAverageLatency') }}</span>
        <strong>{{ averageLabel }}</strong>
      </div>
    </div>
  </el-tooltip>
</template>

<style scoped>
.probe-trend-cell {
  color: #17a169;
  display: inline-grid;
  gap: 3px;
  justify-items: center;
  padding: 4px 0;
  text-align: center;
  width: 156px;
}

.probe-trend-cell.is-failed {
  color: #dc2626;
}

.probe-trend-cell.is-empty {
  color: #94a3b8;
}

.probe-trend-chart {
  color: inherit;
  display: block;
  height: 44px;
  width: 132px;
}

.probe-trend-empty {
  align-items: center;
  color: #86909c;
  display: inline-flex;
  font-size: 12px;
  font-weight: 620;
  height: 44px;
  justify-content: center;
  line-height: 1;
  width: 132px;
}

.probe-trend-baseline {
  stroke: #e5e7eb;
  stroke-width: 1;
}

.probe-trend-bar {
  fill: currentColor;
}

.probe-trend-bar.is-ok {
  fill: #16a34a;
}

.probe-trend-bar.is-slow {
  fill: #f59e0b;
}

.probe-trend-bar.is-very-slow {
  fill: #f97316;
}

.probe-trend-bar.is-failed {
  fill: #dc2626;
}

.probe-trend-bar.is-skipped,
.probe-trend-bar.is-empty {
  fill: #94a3b8;
}

.probe-trend-foot {
  align-items: center;
  color: #667085;
  display: flex;
  font-size: 11px;
  font-weight: 620;
  gap: 4px;
  justify-content: center;
  line-height: 1.1;
  min-width: 0;
}

.probe-trend-foot strong {
  color: #344054;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 760;
  white-space: nowrap;
}

.probe-trend-cell.is-empty .probe-trend-foot strong {
  color: #86909c;
  font-weight: 620;
}

.probe-trend-cell.is-failed .probe-trend-foot strong {
  color: #b42318;
}

:global(.probe-trend-tooltip.el-popper.is-light) {
  border: 1px solid #d8e0ea;
  border-radius: 8px;
  box-shadow: 0 14px 36px rgba(15, 23, 42, 0.16);
  color: #1f2937;
  padding: 10px 12px;
}

.probe-tooltip-content {
  display: grid;
  gap: 8px;
  min-width: 230px;
}

.probe-tooltip-head {
  display: grid;
  gap: 2px;
}

.probe-tooltip-title {
  color: #111827;
  font-size: 13px;
  font-weight: 760;
}

.probe-tooltip-subtitle {
  color: #667085;
  font-size: 12px;
  font-weight: 620;
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.probe-tooltip-grid {
  display: grid;
  gap: 5px 14px;
  grid-template-columns: max-content minmax(0, 1fr);
}

.probe-tooltip-grid span {
  color: #667085;
  font-size: 12px;
  font-weight: 620;
}

.probe-tooltip-grid strong {
  color: #1f2937;
  font-size: 12px;
  font-weight: 720;
  max-width: 260px;
  overflow-wrap: anywhere;
}
</style>
