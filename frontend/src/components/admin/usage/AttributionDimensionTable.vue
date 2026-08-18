<script setup lang="ts">
import { ArrowRight, Tickets } from '@element-plus/icons-vue'
import type {
  ModelUsageStatistics,
  ProjectMemberUsageStatistics,
  ProjectUsageStatistics,
  UserUsageStatistics
} from '../../../api/usage'
import { useBillingCurrency } from '../../../composables/useBillingCurrency'
import { useLocale } from '../../../composables/useLocale'
import { formatNumber } from '../../../utils/format'

export type AttributionDimension = 'project' | 'user' | 'model'
export type AttributionRow =
  | ProjectUsageStatistics
  | UserUsageStatistics
  | ModelUsageStatistics
  | ProjectMemberUsageStatistics

const props = defineProps<{
  kind: AttributionDimension
  rows: AttributionRow[]
  loading: boolean
  primary?: boolean
}>()

const emit = defineEmits<{
  select: [row: AttributionRow]
  details: [row: AttributionRow]
}>()

const { locale, t } = useLocale()
const { formatMoney } = useBillingCurrency()

function rowKey(row: AttributionRow) {
  if (props.kind === 'project') {
    const item = row as ProjectUsageStatistics
    return item.project_id ?? item.project_name
  }
  if (props.kind === 'user') {
    const item = row as UserUsageStatistics | ProjectMemberUsageStatistics
    return item.user_id ?? item.user_display_name
  }
  const item = row as ModelUsageStatistics
  return `${item.channel_id ?? 'channel'}/${item.channel_name}/${item.model}/${item.billing_meter}`
}

function primaryColumnLabel() {
  if (props.kind === 'project') return t('project')
  if (props.kind === 'user') return t('usageUser')
  return t('channelAndModel')
}

function rowTitle(row: AttributionRow) {
  if (props.kind === 'project') return (row as ProjectUsageStatistics).project_name
  if (props.kind === 'user') {
    const item = row as UserUsageStatistics | ProjectMemberUsageStatistics
    return item.user_display_name || item.user_email || item.user_username || '-'
  }
  const item = row as ModelUsageStatistics
  return item.model ? `${item.channel_name || '-'}/${item.model}` : item.channel_name || '-'
}

function rowSubtitle(row: AttributionRow) {
  if (props.kind === 'project') {
    const id = (row as ProjectUsageStatistics).project_id
    return id == null ? '' : `#${id}`
  }
  if (props.kind === 'user') {
    const id = (row as UserUsageStatistics | ProjectMemberUsageStatistics).user_id
    return id == null ? '' : `#${id}`
  }
  const item = row as ModelUsageStatistics
  return item.channel_id == null
    ? ''
    : `#${item.channel_id} / ${billingMeterLabel(item.billing_meter)}`
}

function secondaryCountLabel() {
  if (props.kind === 'user') return t('modelCount')
  return t('userCount')
}

function secondaryCount(row: AttributionRow) {
  if (props.kind === 'project') return (row as ProjectUsageStatistics).member_count
  if (props.kind === 'user') return (row as UserUsageStatistics).model_count
  return (row as ModelUsageStatistics).user_count
}

function billingMeterLabel(value?: string | null) {
  if (value === 'image') return t('billingMeterImageGeneration')
  if (value === 'video') return t('billingMeterVideo')
  if (value === 'audio') return t('billingMeterAudio')
  if (value === 'token') return t('billingMeterToken')
  return t('billingMeterAll')
}

function successRate(success: number, total: number) {
  if (total <= 0) return '-'
  return `${((success / total) * 100).toFixed(1)}%`
}
</script>

<template>
  <el-table
    v-loading="loading"
    class="admin-table service-table attribution-table"
    :data="rows"
    :row-key="rowKey"
    stripe
    :highlight-current-row="primary"
    @row-click="emit('select', $event)"
  >
    <el-table-column :label="primaryColumnLabel()" :min-width="kind === 'model' ? 230 : 190">
      <template #default="{ row }">
        <div class="attribution-primary-cell">
          <strong>{{ rowTitle(row) }}</strong>
          <span v-if="rowSubtitle(row)">{{ rowSubtitle(row) }}</span>
        </div>
      </template>
    </el-table-column>
    <el-table-column :label="t('cost')" min-width="120" align="right">
      <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
    </el-table-column>
    <el-table-column :label="t('requestCount')" min-width="110" align="right">
      <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
    </el-table-column>
    <el-table-column :label="t('successRate')" min-width="100" align="right">
      <template #default="{ row }">{{
        successRate(row.success_count, row.request_count)
      }}</template>
    </el-table-column>
    <el-table-column :label="t('tokens')" min-width="120" align="right">
      <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
    </el-table-column>
    <el-table-column v-if="primary" :label="secondaryCountLabel()" min-width="100" align="right">
      <template #default="{ row }">{{ formatNumber(secondaryCount(row), locale) }}</template>
    </el-table-column>
    <el-table-column :min-width="primary ? 90 : 130" align="right">
      <template #default="{ row }">
        <el-button
          v-if="primary"
          class="icon-only-action"
          :aria-label="t('viewDrilldown')"
          :icon="ArrowRight"
          @click.stop="emit('select', row)"
        />
        <el-button
          v-else
          class="admin-action-button"
          :icon="Tickets"
          @click.stop="emit('details', row)"
        >
          {{ t('details') }}
        </el-button>
      </template>
    </el-table-column>
    <template #empty>
      <el-empty :description="t('noStatisticsData')" />
    </template>
  </el-table>
</template>

<style scoped>
.attribution-table {
  width: 100%;
}

.attribution-primary-cell {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.attribution-primary-cell strong,
.attribution-primary-cell span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-primary-cell strong {
  color: #1d2939;
  font-size: 13px;
  font-weight: 680;
}

.attribution-primary-cell span {
  color: #98a2b3;
  font-size: 12px;
  font-weight: 560;
}
</style>
