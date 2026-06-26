<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { BarChart, LineChart, PieChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent
} from 'echarts/components'
import { init, use, type EChartsCoreOption, type EChartsType } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'

use([
  BarChart,
  LineChart,
  PieChart,
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent,
  CanvasRenderer
])

const props = withDefaults(
  defineProps<{
    option: EChartsCoreOption
    loading?: boolean
    empty?: boolean
    emptyText?: string
    height?: string
  }>(),
  {
    loading: false,
    empty: false,
    emptyText: '',
    height: '300px'
  }
)

const chartEl = ref<HTMLDivElement | null>(null)
let chart: EChartsType | null = null
let resizeObserver: ResizeObserver | null = null

function ensureChart() {
  if (!chartEl.value || chart || props.empty) return
  chart = init(chartEl.value, undefined, { renderer: 'canvas' })
  chart.setOption(props.option, true)
  chart.showLoading('default', { showSpinner: true, text: '' })
  if (!props.loading) chart.hideLoading()
}

function updateChart() {
  void nextTick(() => {
    if (props.empty) {
      chart?.dispose()
      chart = null
      return
    }
    ensureChart()
    chart?.setOption(props.option, true)
    if (props.loading) chart?.showLoading('default', { showSpinner: true, text: '' })
    else chart?.hideLoading()
    chart?.resize()
  })
}

onMounted(() => {
  ensureChart()
  if (chartEl.value) {
    resizeObserver = new ResizeObserver(() => chart?.resize())
    resizeObserver.observe(chartEl.value)
  }
})

watch(() => props.option, updateChart, { deep: true })
watch(() => props.loading, updateChart)
watch(() => props.empty, updateChart)

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
  chart?.dispose()
  chart = null
})
</script>

<template>
  <div class="admin-usage-chart" :style="{ height }">
    <div v-if="empty" class="admin-usage-chart-empty">
      {{ emptyText }}
    </div>
    <div v-else ref="chartEl" class="admin-usage-chart-canvas"></div>
  </div>
</template>

<style scoped>
.admin-usage-chart {
  min-height: 220px;
  position: relative;
  width: 100%;
}

.admin-usage-chart-canvas {
  height: 100%;
  width: 100%;
}

.admin-usage-chart-empty {
  align-items: center;
  border: 1px dashed #d6dee8;
  border-radius: 8px;
  color: #98a2b3;
  display: flex;
  font-size: 13px;
  font-weight: 600;
  height: 100%;
  justify-content: center;
  min-height: 220px;
}
</style>
