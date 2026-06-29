<script setup lang="ts">
import { CircleCheck, CircleCloseFilled, PriceTag, Warning } from '@element-plus/icons-vue'
import { useLocale } from '../../../composables/useLocale'
import type { Channel } from '../../../types/admin'

export type ChannelExpandPriceRow = {
  model: string
  price: string
  cachePrice: string
  missing?: boolean
  disabled?: boolean
  billingEnabled?: boolean
  runtimeStatus: string
  runtimeStatusLabel: string
  runtimeEnabled: boolean
  runtimeToggleDisabled: boolean
  upstreamMissing?: boolean
}

defineProps<{
  channel: Channel
  rows: ChannelExpandPriceRow[]
}>()

const emit = defineEmits<{
  configurePrice: [channel: Channel]
  toggleModelRuntime: [channelId: number, model: string, enabled: boolean]
}>()

const { t } = useLocale()
</script>

<template>
  <div class="channel-expand-panel" :class="{ 'is-channel-disabled': !channel.enabled }">
    <div class="channel-expand-head">
      <div>
        <strong>{{ t('modelPriceDetails') }}</strong>
      </div>
      <el-button
        class="admin-action-button expand-price-action"
        :icon="PriceTag"
        @click="emit('configurePrice', channel)"
      >
        {{ t('configurePrice') }}
      </el-button>
    </div>
    <div class="channel-expand-price-table">
      <div class="channel-expand-price-row is-head">
        <span>{{ t('modelName') }}</span>
        <span class="channel-head-label">{{ t('inputOutputPriceShort') }}</span>
        <span class="channel-head-label">{{ t('cacheReadWritePriceShort') }}</span>
        <span>{{ t('priceStatus') }}</span>
        <span>{{ t('runtimeStatus') }}</span>
        <span>{{ t('modelRuntimeSwitch') }}</span>
      </div>
      <div
        v-for="item in rows"
        :key="item.model"
        class="channel-expand-price-row"
        :class="{
          'is-missing': item.missing,
          'is-disabled': item.disabled,
          'is-upstream-missing': item.upstreamMissing
        }"
      >
        <span class="channel-price-model">{{ item.model }}</span>
        <span class="channel-detail-price">{{ item.price }}</span>
        <span class="channel-detail-price">{{ item.cachePrice }}</span>
        <span
          class="channel-detail-status"
          :class="{ 'is-missing': item.missing, 'is-disabled': !item.billingEnabled }"
          :aria-label="
            item.missing ? t('priceMissing') : item.billingEnabled ? t('enabled') : t('disabled')
          "
        >
          <el-icon>
            <Warning v-if="item.missing" />
            <CircleCheck v-else-if="item.billingEnabled" />
            <CircleCloseFilled v-else />
          </el-icon>
        </span>
        <span class="channel-detail-runtime-raw" :class="`is-${item.runtimeStatus}`">
          {{ item.runtimeStatusLabel }}
        </span>
        <span
          class="channel-detail-runtime-switch"
          :aria-label="
            item.upstreamMissing
              ? t('modelUpstreamMissing')
              : item.runtimeEnabled
                ? t('enabled')
                : t('disabled')
          "
        >
          <el-switch
            :model-value="item.runtimeEnabled"
            :disabled="item.runtimeToggleDisabled"
            size="small"
            @change="emit('toggleModelRuntime', channel.id, item.model, Boolean($event))"
          />
          <span class="channel-detail-switch-copy">
            <strong>
              {{
                item.upstreamMissing
                  ? t('modelUpstreamMissing')
                  : item.runtimeEnabled
                    ? t('enabled')
                    : t('disabled')
              }}
            </strong>
          </span>
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.channel-expand-panel {
  display: grid;
  gap: 12px;
  margin: 0;
  padding: 14px 16px 16px 60px;
}

.channel-expand-head {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
}

.channel-expand-head div {
  display: grid;
  gap: 4px;
  min-width: 0;
}

.channel-expand-head strong {
  color: #1d2129;
  font-size: 14px;
  font-weight: 760;
  line-height: 1.2;
  white-space: nowrap;
}

.expand-price-action.el-button {
  --el-button-bg-color: var(--admin-primary);
  --el-button-border-color: var(--admin-primary);
  --el-button-hover-bg-color: var(--admin-primary-hover);
  --el-button-hover-border-color: var(--admin-primary-hover);
  --el-button-hover-text-color: #ffffff;
  --el-button-text-color: #ffffff;
  box-shadow: none;
}

.expand-price-action.el-button:not(.is-disabled):hover,
.expand-price-action.el-button:not(.is-disabled):focus,
.expand-price-action.el-button:not(.is-disabled):active {
  background-color: var(--admin-primary-hover);
  border-color: var(--admin-primary-hover);
  color: #ffffff;
  box-shadow: none;
}

.channel-expand-price-table {
  background: #ffffff;
  border: 1px solid #e3ebf4;
  border-radius: 8px;
  overflow-x: auto;
  overflow-y: hidden;
}

.channel-expand-price-row {
  align-items: center;
  background: #ffffff;
  column-gap: 12px;
  display: grid;
  row-gap: 8px;
  grid-template-columns:
    minmax(120px, 1.5fr)
    minmax(160px, 0.7fr)
    minmax(160px, 0.7fr)
    92px
    56px
    132px;
  min-height: 46px;
  padding: 0 12px;
}

.channel-expand-price-row + .channel-expand-price-row {
  border-top: 1px solid #eef3f8;
}

.channel-expand-price-row.is-head {
  background: #f4f7fb;
  color: #4e5969;
  font-size: 12px;
  font-weight: 760;
  min-height: 38px;
}

.channel-expand-price-row.is-head span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-head-label {
  align-items: center;
  display: inline-flex;
  justify-content: flex-end;
  min-width: 0;
}

.channel-expand-price-row.is-head span:nth-child(2),
.channel-expand-price-row.is-head span:nth-child(3),
.channel-detail-price {
  text-align: right;
}

.channel-expand-price-row.is-head span:nth-child(4),
.channel-expand-price-row.is-head span:nth-child(5),
.channel-expand-price-row.is-head span:nth-child(6) {
  text-align: center;
}

.channel-expand-price-row.is-head span:nth-child(4),
.channel-detail-status {
  padding-left: 12px;
}

.channel-expand-price-row.is-missing,
.channel-expand-price-row.is-upstream-missing {
  background: #fffaf3;
}

.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing) {
  background: #f8fafc;
}

.channel-expand-price-row.is-missing .channel-price-model,
.channel-expand-price-row.is-upstream-missing .channel-price-model {
  color: #c2410c;
}

.channel-expand-panel.is-channel-disabled .channel-expand-head strong {
  color: #94a3b8;
}

.channel-expand-panel.is-channel-disabled .channel-expand-price-row {
  color: #94a3b8;
}

.channel-expand-panel.is-channel-disabled .channel-expand-price-row.is-head {
  background: #f1f5f9;
}

.channel-expand-panel.is-channel-disabled .channel-expand-price-row .channel-price-model,
.channel-expand-panel.is-channel-disabled .channel-expand-price-row .channel-detail-price {
  color: #94a3b8;
}

.channel-expand-panel.is-channel-disabled .channel-detail-status .el-icon,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-status
  .el-icon {
  color: #cbd5e1;
}

.channel-expand-panel.is-channel-disabled .channel-detail-runtime-raw {
  color: #94a3b8;
}

.channel-expand-panel.is-channel-disabled .channel-detail-runtime-switch :deep(.el-switch) {
  --el-switch-off-color: #cbd5e1;
  --el-switch-on-color: #cbd5e1;
}

.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-price-model,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-price,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-status,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-runtime-raw,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-runtime-switch {
  color: #94a3b8;
}

.channel-expand-price-row .channel-price-model {
  background: transparent;
  border: 0;
  border-radius: 0;
  color: #1d2129;
  display: inline-block;
  font-size: 13px;
  font-weight: 400;
  max-width: 100%;
  overflow: hidden;
  padding: 0;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-detail-price {
  align-items: center;
  color: #1d2129;
  display: inline-flex;
  font-feature-settings: 'tnum';
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  font-weight: 400;
  justify-content: flex-end;
  white-space: nowrap;
}

.channel-detail-status {
  align-items: center;
  display: inline-flex;
  justify-content: center;
  justify-self: center;
  padding-left: 20px;
}

.channel-detail-status .el-icon {
  align-items: center;
  color: #22c55e;
  display: inline-flex;
  font-size: 18px;
  justify-content: center;
}

.channel-detail-status.is-missing .el-icon {
  color: #f97316;
}

.channel-detail-runtime-raw {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 999px;
  color: #475569;
  display: inline-flex;
  font-size: 12px;
  font-weight: 700;
  justify-content: center;
  line-height: 1;
  padding: 2px 5px;
  white-space: nowrap;
}

.channel-detail-runtime-raw.is-normal {
  background: #f0fdf4;
  border-color: #bbf7d0;
  color: #15803d;
}

.channel-detail-runtime-raw.is-cooldown {
  background: var(--admin-primary-soft);
  border-color: var(--admin-primary-border);
  color: var(--admin-primary);
}

.channel-detail-runtime-raw.is-failed {
  background: #fff1f2;
  border-color: #fecdd3;
  color: #dc2626;
}

.channel-detail-runtime-switch {
  align-items: center;
  display: inline-grid;
  gap: 6px;
  grid-template-columns: auto minmax(0, 1fr);
  justify-content: center;
  justify-self: center;
}

.channel-detail-runtime-switch :deep(.el-switch) {
  --el-switch-off-color: #94a3b8;
  --el-switch-on-color: #22c55e;
}

.channel-detail-switch-copy {
  display: inline-flex;
  line-height: 1.1;
  min-width: 0;
  text-align: center;
}

.channel-detail-switch-copy strong {
  color: #344054;
  font-size: 12px;
  font-weight: 700;
  white-space: nowrap;
}

.channel-expand-price-row.is-upstream-missing .channel-detail-switch-copy strong,
.channel-expand-price-row.is-missing .channel-detail-price,
.channel-expand-price-row.is-missing .channel-detail-status {
  color: #c2410c;
}

@media (max-width: 760px) {
  .channel-expand-panel {
    padding: 12px;
  }

  .channel-expand-head {
    align-items: stretch;
    display: grid;
  }

  .channel-expand-price-row {
    gap: 8px;
    grid-template-columns: 1fr;
    padding: 12px;
  }

  .channel-expand-price-row.is-head {
    display: none;
  }

  .channel-expand-price-row span,
  .channel-detail-price,
  .channel-detail-status {
    text-align: left;
  }
}
</style>
