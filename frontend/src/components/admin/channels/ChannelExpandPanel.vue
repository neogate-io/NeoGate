<script setup lang="ts">
import { CircleCheck, CircleCloseFilled, Tickets, Warning } from '@element-plus/icons-vue'
import { computed, ref, watch } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import type { Channel } from '../../../types/admin'

export type ChannelExpandPriceGroup = {
  label: string
  price: string
  inline?: boolean
}

export type ChannelExpandVideoTierRow = {
  specs: string
  price: string
  priceGroups?: ChannelExpandPriceGroup[]
}

export type ChannelExpandPriceRow = {
  model: string
  category: 'text' | 'image' | 'video'
  billingMeterLabel: string
  price: string
  cachePrice: string
  imagePriceGroups?: ChannelExpandPriceGroup[]
  videoBillingMode?: string
  videoTiers?: ChannelExpandVideoTierRow[]
  missing?: boolean
  disabled?: boolean
  billingEnabled?: boolean
  runtimeStatus: string
  runtimeStatusLabel: string
  runtimeEnabled: boolean
  runtimeToggleDisabled: boolean
  upstreamMissing?: boolean
}

const props = defineProps<{
  channel: Channel
  rows: ChannelExpandPriceRow[]
}>()

const emit = defineEmits<{
  toggleModelRuntime: [channelId: number, model: string, enabled: boolean]
  editPrice: [channel: Channel]
}>()

const { t } = useLocale()
const activeTab = ref('text')

const modelSections = computed(() =>
  [
    {
      key: 'text',
      title: t('textModelPrices'),
      rows: props.rows.filter((row) => row.category === 'text')
    },
    {
      key: 'image',
      title: t('imageModelPrices'),
      rows: props.rows.filter((row) => row.category === 'image')
    },
    {
      key: 'video',
      title: t('videoModelPrices'),
      rows: props.rows.filter((row) => row.category === 'video')
    }
  ].filter((section) => section.rows.length > 0)
)

watch(
  modelSections,
  () => {
    if (modelSections.value.some((section) => section.key === activeTab.value)) return
    activeTab.value = modelSections.value[0]?.key ?? 'text'
  },
  { immediate: true }
)

function videoTiersForDisplay(row: ChannelExpandPriceRow) {
  return row.videoTiers?.length
    ? row.videoTiers
    : [{ specs: t('videoTierAnyResolution'), price: t('noKnownVideoTiers') }]
}
</script>

<template>
  <div class="channel-expand-panel" :class="{ 'is-channel-disabled': !channel.enabled }">
    <div class="channel-expand-actions">
      <el-button
        class="admin-action-button channel-expand-price-action"
        type="primary"
        :aria-label="t('editModelPrices')"
        :icon="Tickets"
        @click="emit('editPrice', channel)"
      >
        {{ t('editModelPrices') }}
      </el-button>
    </div>
    <el-tabs v-if="modelSections.length" v-model="activeTab" class="channel-model-tabs">
      <el-tab-pane v-for="section in modelSections" :key="section.key" :name="section.key">
        <template #label>
          <span class="channel-model-tab-label">
            {{ section.title }}
            <span>{{ section.rows.length }}</span>
          </span>
        </template>
        <div v-if="section.key === 'video'" class="channel-expand-video-editor">
          <div class="channel-expand-video-head">
            <span>{{ t('modelName') }}</span>
            <span>{{ t('billingMeter') }}</span>
            <span>{{ t('videoTierResolutions') }}</span>
            <span>{{ t('modelPrice') }}</span>
            <span>{{ t('priceStatus') }}</span>
            <span>{{ t('runtimeStatus') }}</span>
            <span>{{ t('modelRuntimeSwitch') }}</span>
          </div>
          <div
            v-for="item in section.rows"
            :key="item.model"
            class="channel-expand-video-model-row"
            :class="{
              'is-missing': item.missing,
              'is-disabled': item.disabled,
              'is-upstream-missing': item.upstreamMissing
            }"
          >
            <span class="channel-price-model">{{ item.model }}</span>
            <span class="channel-detail-meter">{{ item.videoBillingMode }}</span>
            <div class="channel-video-tier-stack">
              <div
                v-for="(tier, tierIndex) in videoTiersForDisplay(item)"
                :key="`${item.model}:${tierIndex}`"
                class="channel-video-tier-row"
              >
                <span class="channel-detail-price">{{ tier.specs }}</span>
                <div v-if="tier.priceGroups?.length" class="channel-video-price-cell">
                  <div
                    v-for="group in tier.priceGroups"
                    :key="group.label"
                    class="channel-image-price-group"
                    :class="{ 'is-inline': group.inline }"
                  >
                    <span class="channel-image-price-value">{{ group.price }}</span>
                    <span class="channel-image-price-label">{{ group.label }}</span>
                  </div>
                </div>
                <span v-else class="channel-detail-price">{{ tier.price }}</span>
              </div>
            </div>
            <span
              class="channel-detail-status"
              :class="{ 'is-missing': item.missing, 'is-disabled': !item.billingEnabled }"
              :aria-label="
                item.missing
                  ? t('priceMissing')
                  : item.billingEnabled
                    ? t('enabled')
                    : t('disabled')
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

        <div
          v-else
          class="channel-expand-price-table"
          :class="{ 'is-image-table': section.key === 'image' }"
        >
          <div class="channel-expand-price-row is-head">
            <span>{{ t('modelName') }}</span>
            <span v-if="section.key === 'image'">{{ t('billingMeter') }}</span>
            <span v-if="section.key === 'image'">{{ t('modelPrice') }}</span>
            <template v-else>
              <span class="channel-head-label">{{ t('inputOutputPriceShort') }}</span>
              <span class="channel-head-label">{{ t('cacheReadWritePriceShort') }}</span>
            </template>
            <span>{{ t('priceStatus') }}</span>
            <span>{{ t('runtimeStatus') }}</span>
            <span>{{ t('modelRuntimeSwitch') }}</span>
          </div>
          <div
            v-for="item in section.rows"
            :key="item.model"
            class="channel-expand-price-row"
            :class="{
              'is-missing': item.missing,
              'is-disabled': item.disabled,
              'is-upstream-missing': item.upstreamMissing
            }"
          >
            <span class="channel-price-model">{{ item.model }}</span>
            <span v-if="section.key === 'image'" class="channel-detail-meter">
              {{ item.billingMeterLabel }}
            </span>
            <div v-if="section.key === 'image'" class="channel-image-price-cell">
              <div
                v-for="group in item.imagePriceGroups"
                :key="group.label"
                class="channel-image-price-group"
                :class="{ 'is-inline': group.inline }"
              >
                <span class="channel-image-price-value">{{ group.price }}</span>
                <span class="channel-image-price-label">{{ group.label }}</span>
              </div>
            </div>
            <template v-else>
              <span class="channel-detail-price">{{ item.price }}</span>
              <span class="channel-detail-price">{{ item.cachePrice }}</span>
            </template>
            <span
              class="channel-detail-status"
              :class="{ 'is-missing': item.missing, 'is-disabled': !item.billingEnabled }"
              :aria-label="
                item.missing
                  ? t('priceMissing')
                  : item.billingEnabled
                    ? t('enabled')
                    : t('disabled')
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
      </el-tab-pane>
    </el-tabs>
  </div>
</template>

<style scoped>
.channel-expand-panel {
  background: #ffffff;
  display: grid;
  gap: 12px;
  margin: 0;
  padding: 14px 16px 16px 60px;
  position: relative;
}

.channel-expand-actions {
  align-items: center;
  display: flex;
  justify-content: flex-end;
  min-width: 0;
  position: absolute;
  right: 16px;
  top: 14px;
  z-index: 1;
}

.channel-expand-price-action {
  min-height: 30px;
}

.channel-model-tabs :deep(.el-tabs__header) {
  margin: 0 0 10px;
  padding-right: 120px;
}

.channel-model-tabs :deep(.el-tabs__nav-wrap::after) {
  display: none;
}

.channel-model-tab-label {
  align-items: center;
  display: inline-flex;
  gap: 7px;
}

.channel-model-tab-label span {
  align-items: center;
  background: #eef6ff;
  border: 1px solid #d2e8ff;
  border-radius: 999px;
  color: var(--admin-primary);
  display: inline-flex;
  font-size: 11px;
  font-weight: 620;
  justify-content: center;
  line-height: 1;
  min-width: 24px;
  padding: 3px 8px;
}

.channel-expand-price-table {
  background: #ffffff;
  border: 1px solid #e3ebf4;
  border-radius: 8px;
  overflow-x: auto;
  overflow-y: hidden;
}

.channel-expand-video-editor {
  background: #ffffff;
  border: 1px solid #e3ebf4;
  border-radius: 8px;
  overflow-x: auto;
  overflow-y: hidden;
}

.channel-expand-video-head,
.channel-expand-video-model-row {
  display: grid;
  grid-template-columns:
    minmax(190px, 1fr)
    128px
    128px
    240px
    92px
    76px
    132px;
  min-width: 1012px;
}

.channel-expand-video-head {
  align-items: center;
  background: #f4f7fb;
  border-bottom: 1px solid #e2e8f0;
  color: #4e5969;
  font-size: 12px;
  font-weight: 760;
  min-height: 38px;
}

.channel-expand-video-head > span,
.channel-expand-video-model-row > .channel-price-model,
.channel-expand-video-model-row > .channel-detail-meter,
.channel-expand-video-model-row > .channel-detail-status,
.channel-expand-video-model-row > .channel-detail-runtime-raw,
.channel-expand-video-model-row > .channel-detail-runtime-switch,
.channel-video-tier-row > span {
  min-width: 0;
  padding: 0 10px;
}

.channel-expand-video-head > span {
  overflow: hidden;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-expand-video-head > span:first-child {
  text-align: left;
}

.channel-expand-video-head > span:nth-child(2),
.channel-expand-video-head > span:nth-child(3),
.channel-expand-video-head > span:nth-child(4),
.channel-expand-video-model-row > .channel-detail-meter {
  justify-content: center;
  text-align: center;
}

.channel-expand-video-model-row > .channel-detail-status,
.channel-expand-video-model-row > .channel-detail-runtime-raw,
.channel-expand-video-model-row > .channel-detail-runtime-switch {
  align-self: center;
  justify-self: center;
}

.channel-expand-video-model-row {
  align-items: stretch;
  background: #ffffff;
  min-height: 54px;
}

.channel-expand-video-model-row + .channel-expand-video-model-row {
  border-top: 1px solid #eef3f8;
}

.channel-video-tier-stack {
  display: grid;
  grid-column: 3 / 5;
  min-width: 0;
}

.channel-video-tier-row {
  align-items: center;
  display: grid;
  grid-template-columns: 128px 240px;
  min-height: 54px;
}

.channel-video-tier-row > .channel-detail-price {
  justify-content: center;
  text-align: center;
}

.channel-video-tier-row + .channel-video-tier-row {
  border-top: 1px solid #edf2f7;
}

.channel-video-price-cell {
  align-items: center;
  display: flex;
  gap: 14px;
  justify-content: center;
  min-width: 0;
  padding: 0 10px;
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

.channel-expand-price-table.is-image-table .channel-expand-price-row {
  grid-template-columns:
    minmax(120px, 1.4fr)
    104px
    minmax(240px, 1fr)
    92px
    56px
    132px;
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

.channel-expand-price-table.is-image-table .channel-expand-price-row.is-head span:nth-child(2),
.channel-expand-price-table.is-image-table .channel-expand-price-row.is-head span:nth-child(3),
.channel-expand-price-table.is-image-table .channel-expand-price-row.is-head span:nth-child(4),
.channel-expand-price-table.is-image-table .channel-expand-price-row.is-head span:nth-child(5),
.channel-expand-price-table.is-image-table .channel-expand-price-row.is-head span:nth-child(6) {
  text-align: center;
}

.channel-expand-price-table.is-image-table .channel-expand-price-row > .channel-detail-meter {
  justify-content: center;
  text-align: center;
}

.channel-image-price-cell {
  align-items: center;
  display: flex;
  gap: 16px;
  justify-content: center;
  min-width: 0;
}

.channel-image-price-group {
  align-items: center;
  display: grid;
  gap: 2px;
  justify-items: center;
  min-width: 0;
}

.channel-image-price-group.is-inline {
  display: inline-flex;
  gap: 4px;
}

.channel-image-price-value {
  color: #1d2129;
  font-feature-settings: 'tnum';
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  font-weight: 500;
  line-height: 1.2;
  white-space: nowrap;
}

.channel-image-price-label {
  color: #718096;
  font-size: 10.5px;
  font-weight: 500;
  line-height: 1.1;
  white-space: nowrap;
}

.channel-image-price-group.is-inline .channel-image-price-label {
  font-size: 12px;
}

.channel-expand-price-row.is-head span:nth-child(4),
.channel-detail-status {
  padding-left: 12px;
}

.channel-expand-price-row.is-missing,
.channel-expand-price-row.is-upstream-missing,
.channel-expand-video-model-row.is-missing,
.channel-expand-video-model-row.is-upstream-missing {
  background: #fffaf3;
}

.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing),
.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing) {
  background: #f8fafc;
}

.channel-expand-price-row.is-missing .channel-price-model,
.channel-expand-price-row.is-upstream-missing .channel-price-model,
.channel-expand-video-model-row.is-missing .channel-price-model,
.channel-expand-video-model-row.is-upstream-missing .channel-price-model {
  color: #c2410c;
}

.channel-expand-panel.is-channel-disabled .channel-expand-price-row {
  color: #94a3b8;
}

.channel-expand-panel.is-channel-disabled .channel-expand-video-model-row {
  color: #94a3b8;
}

.channel-expand-panel.is-channel-disabled .channel-expand-price-row.is-head {
  background: #f1f5f9;
}

.channel-expand-panel.is-channel-disabled .channel-expand-video-head {
  background: #f1f5f9;
}

.channel-expand-panel.is-channel-disabled .channel-expand-price-row .channel-price-model,
.channel-expand-panel.is-channel-disabled .channel-expand-price-row .channel-detail-price,
.channel-expand-panel.is-channel-disabled .channel-expand-video-model-row .channel-price-model,
.channel-expand-panel.is-channel-disabled .channel-expand-video-model-row .channel-detail-price,
.channel-expand-panel.is-channel-disabled .channel-expand-video-model-row .channel-detail-meter {
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
  .channel-detail-meter,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-image-price-value,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-image-price-label,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-status,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-runtime-raw,
.channel-expand-price-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-runtime-switch {
  color: #94a3b8;
}

.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-price-model,
.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-price,
.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-meter,
.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-status,
.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
  .channel-detail-runtime-raw,
.channel-expand-video-model-row.is-disabled:not(.is-missing):not(.is-upstream-missing)
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

.channel-expand-video-model-row .channel-price-model,
.channel-detail-meter {
  align-items: center;
  color: #1d2129;
  display: inline-flex;
  font-size: 13px;
  font-weight: 400;
  min-width: 0;
  overflow: hidden;
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
  height: 22px;
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

  .channel-expand-actions {
    justify-content: flex-start;
    position: static;
  }

  .channel-model-tabs :deep(.el-tabs__header) {
    margin: 0 0 10px;
    padding-right: 0;
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
