<script setup lang="ts">
import ProviderIcon from '../../common/ProviderIcon.vue'
import { useLocale } from '../../../composables/useLocale'
import type { BillingMeter } from '../../../types/admin'

export type ChannelPriceForm = {
  provider: string
  model: string
  billingMeter: BillingMeter | null
  inputUsdPerMillion: number
  outputUsdPerMillion: number
  cacheReadUsdPerMillion: number
  cacheWriteUsdPerMillion: number | null
  unitUsd: number
  enabled: boolean
  hasPrice: boolean
  hasPriceRecord: boolean
  billingMeterLocked: boolean
  canUseImageBilling: boolean
  templateSource?: string
}

const open = defineModel<boolean>('open', { required: true })

defineProps<{
  forms: Record<string, ChannelPriceForm>
  saving: boolean
  hasReferencePrice: (form: ChannelPriceForm) => boolean
  referencePriceSummary: (form: ChannelPriceForm) => string
  referencePriceFallbackLabel: (form: ChannelPriceForm) => string
  priceIconProvider: (form: ChannelPriceForm) => string
}>()

const emit = defineEmits<{
  applyReferencePrices: []
  save: []
}>()

const { t } = useLocale()

function formatUsdInput(value: number | string) {
  if (value === '' || value === undefined || value === null) return ''
  return `$${value}`
}

function parseUsdInput(value: string) {
  return value.replace(/^\$/, '')
}
</script>

<template>
  <el-dialog
    v-model="open"
    class="channel-dialog price-dialog"
    :title="t('configurePrice')"
    width="min(860px, calc(100vw - 32px))"
  >
    <div class="price-editor">
      <div class="price-editor-head">
        <span>{{ t('model') }}</span>
        <span>{{ t('billingMeter') }}</span>
        <div class="price-editor-head-label">
          <strong>{{ t('tokenPricePair') }}</strong>
          <small>{{ t('inputOutputPair') }}/{{ t('pricePerMillionTokens') }}</small>
        </div>
        <div class="price-editor-head-label">
          <strong>{{ t('cachePricePair') }}</strong>
          <small>{{ t('readWritePair') }}/{{ t('pricePerMillionTokens') }}</small>
        </div>
        <span>{{ t('officialReferencePrice') }}</span>
      </div>

      <div class="price-editor-body">
        <div
          v-for="row in Object.values(forms)"
          :key="`${row.provider}:${row.model}`"
          class="price-editor-row"
        >
          <div class="price-model-cell" :title="row.model">
            <ProviderIcon :provider="priceIconProvider(row)" class="price-model-icon" />
            <span>{{ row.model }}</span>
          </div>
          <div class="price-meter-cell">
            <span v-if="row.billingMeterLocked" class="price-meter-static">
              {{
                row.billingMeter === 'image'
                  ? t('billingMeterImageGeneration')
                  : t('billingMeterToken')
              }}
            </span>
            <el-select
              v-else
              v-model="row.billingMeter"
              class="price-meter-select"
              :placeholder="t('billingMeterRequired')"
            >
              <el-option :label="t('billingMeterToken')" value="token" />
              <el-option
                v-if="row.canUseImageBilling"
                :label="t('billingMeterImageGeneration')"
                value="image"
              />
            </el-select>
          </div>
          <div class="price-pair-field">
            <div v-if="row.billingMeter === 'token'" class="price-pair-input">
              <el-input-number
                v-model="row.inputUsdPerMillion"
                class="price-number-input"
                :controls="false"
                :formatter="formatUsdInput"
                :min="0"
                :parser="parseUsdInput"
                :step="0.01"
              />
              <span class="price-pair-separator">/</span>
              <el-input-number
                v-model="row.outputUsdPerMillion"
                class="price-number-input"
                :controls="false"
                :formatter="formatUsdInput"
                :min="0"
                :parser="parseUsdInput"
                :step="0.01"
              />
            </div>
            <div v-else-if="row.billingMeter === 'image'" class="price-single-input">
              <el-input-number
                v-model="row.unitUsd"
                class="price-number-input"
                :controls="false"
                :formatter="formatUsdInput"
                :min="0"
                :parser="parseUsdInput"
                :step="0.01"
              />
              <span class="price-unit-label">{{ t('perImage') }}</span>
            </div>
            <span v-else class="price-muted-cell">{{ t('billingMeterRequired') }}</span>
          </div>
          <div class="price-pair-field">
            <div v-if="row.billingMeter === 'token'" class="price-pair-input">
              <el-input-number
                v-model="row.cacheReadUsdPerMillion"
                class="price-number-input"
                :controls="false"
                :formatter="formatUsdInput"
                :min="0"
                :parser="parseUsdInput"
                :step="0.01"
              />
              <span class="price-pair-separator">/</span>
              <el-input-number
                v-model="row.cacheWriteUsdPerMillion"
                class="price-number-input"
                :controls="false"
                :formatter="formatUsdInput"
                :min="0"
                :parser="parseUsdInput"
                :step="0.01"
              />
            </div>
            <span v-else class="price-muted-cell">-</span>
          </div>
          <div class="reference-price-cell">
            <template v-if="hasReferencePrice(row)">
              <span class="reference-price-summary">{{ referencePriceSummary(row) }}</span>
              <span v-if="row.templateSource" class="reference-price-source">
                {{ row.templateSource }}
              </span>
            </template>
            <el-tag
              v-else
              class="reference-price-fallback-tag"
              :type="row.hasPrice ? 'info' : 'warning'"
            >
              {{ referencePriceFallbackLabel(row) }}
            </el-tag>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="dialog-footer price-dialog-footer">
        <el-button :loading="saving" @click="emit('applyReferencePrices')">
          {{ t('applyReferencePrices') }}
        </el-button>
        <div class="price-dialog-actions">
          <el-button @click="open = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="saving" @click="emit('save')">
            {{ t('save') }}
          </el-button>
        </div>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.price-editor {
  border: 1px solid #dfe6ef;
  border-radius: 7px;
  overflow: hidden;
}

.price-editor-head,
.price-editor-row {
  align-items: center;
  display: grid;
  grid-template-columns:
    minmax(120px, 1fr)
    106px
    144px
    144px
    minmax(132px, 0.9fr);
}

.price-editor-head {
  background: #f8fafc;
  border-bottom: 1px solid #e2e8f0;
  color: #556274;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.3;
  min-height: 42px;
}

.price-editor-head > span,
.price-editor-head-label,
.price-editor-row > * {
  min-width: 0;
  padding: 0 8px;
}

.price-editor-head-label {
  display: grid;
  gap: 3px;
}

.price-editor-head-label strong {
  color: #334155;
  font-size: 13px;
  font-weight: 760;
  line-height: 1.1;
}

.price-editor-head-label small {
  color: #7a8797;
  font-size: 11px;
  font-weight: 580;
  line-height: 1.15;
  white-space: nowrap;
}

.price-editor-body {
  max-height: min(320px, 58vh);
  overflow: auto;
}

.price-editor-row {
  background: #ffffff;
  min-height: 56px;
}

.price-editor-row + .price-editor-row {
  border-top: 1px solid #edf2f7;
}

.price-editor-row:nth-child(odd) {
  background: #fbfdff;
}

.price-model-cell {
  align-items: center;
  color: #182132;
  display: flex;
  font-size: 13px;
  font-weight: 600;
  gap: 7px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.price-model-cell span {
  overflow: hidden;
  text-overflow: ellipsis;
}

.price-model-icon {
  border-radius: 5px;
  flex: 0 0 auto;
  height: 20px;
  width: 20px;
}

.price-pair-field {
  display: flex;
  justify-content: flex-start;
  min-width: 0;
}

.price-meter-select {
  width: 96px;
}

.price-meter-static {
  align-items: center;
  background: #f5f7fb;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: #5f6f85;
  display: inline-flex;
  font-size: 12px;
  font-weight: 680;
  justify-content: center;
  line-height: 1.2;
  min-width: 58px;
  padding: 5px 10px;
}

.price-pair-input,
.price-single-input {
  align-items: center;
  background: #ffffff;
  border: 1px solid #d8e0ec;
  border-radius: 6px;
  display: flex;
  gap: 3px;
  min-height: 34px;
  padding: 0 5px;
  width: 128px;
}

.price-single-input {
  width: 136px;
}

.price-muted-cell,
.price-unit-label {
  color: #7a8797;
  font-size: 12px;
  font-weight: 620;
  white-space: nowrap;
}

.price-number-input {
  flex: 0 1 53px;
  min-width: 0;
  width: 53px;
}

.price-pair-separator {
  color: #7b8797;
  flex: 0 0 auto;
  font-size: 15px;
  font-weight: 400;
  line-height: 1;
}

.price-number-input :deep(.el-input__wrapper) {
  border-radius: 0;
  box-shadow: none;
  min-height: 32px;
  padding: 0;
}

.price-number-input :deep(.el-input__inner) {
  color: #1f2937;
  font-size: 13px;
  font-weight: 500;
  text-align: right;
}

.reference-price-cell {
  align-items: flex-start;
  color: #64748b;
  display: grid;
  gap: 1px;
  line-height: 1.25;
}

.reference-price-summary {
  color: #475569;
  font-size: 11px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: pre-line;
}

.reference-price-source {
  color: var(--brand-blue);
  font-size: 11px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.reference-price-fallback-tag {
  animation: none;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 650;
  justify-self: start;
  line-height: 1.1;
  max-width: 108px;
  min-width: 84px;
  padding: 0 12px;
  transition: none;
}

.reference-price-fallback-tag :deep(.el-tag__content) {
  display: block;
  overflow: hidden;
  text-align: center;
  text-overflow: ellipsis;
  transition: none;
  white-space: nowrap;
}

.dialog-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.price-dialog-footer {
  align-items: center;
  justify-content: space-between;
}

.price-dialog-actions {
  display: flex;
  gap: 12px;
}

:global(.channel-dialog) {
  border-radius: 8px;
  max-width: calc(100vw - 32px);
}

:global(.price-dialog .el-dialog__header) {
  margin: 0;
  padding: 18px 22px 14px;
}

:global(.price-dialog .el-dialog__title) {
  color: #111827;
  font-size: 18px;
  font-weight: 760;
  line-height: 1.2;
}

:global(.price-dialog .el-dialog__headerbtn) {
  right: 12px;
  top: 10px;
}

:global(.price-dialog .el-dialog__body) {
  padding: 18px 22px;
}

:global(.price-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 22px 18px;
}

:global(.price-dialog .dialog-footer .el-button) {
  border-radius: 7px;
  font-weight: 680;
  min-height: 34px;
  min-width: 70px;
}

@media (max-width: 760px) {
  .price-editor-head {
    display: none;
  }

  .price-editor {
    border-radius: 8px;
  }

  .price-editor-body {
    max-height: none;
  }

  .price-editor-row {
    align-items: stretch;
    gap: 10px;
    grid-template-columns: 1fr;
    padding: 14px;
  }

  .price-editor-row > * {
    padding: 0;
  }

  .price-dialog-footer {
    align-items: stretch;
    display: grid;
    grid-template-columns: 1fr;
  }

  .price-dialog-actions {
    display: grid;
    gap: 10px;
    grid-template-columns: 1fr 1fr;
  }
}
</style>
