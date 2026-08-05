<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import { useBillingCurrency } from '../../../composables/useBillingCurrency'
import type { BillingMeter, VideoBillingMode } from '../../../types/admin'
import { resolvedVideoTokensPerSecondEstimate } from '../../../utils/pricing'

export type ChannelVideoPriceTierForm = {
  resolutionsText: string
  resolutionLabel?: string
  pricePairLeftLabel?: string
  pricePairRightLabel?: string
  inputWithVideo: number
  inputWithoutVideo: number
  estimatedTokensPerSecond: number
  inputWithVideoUnit: number
  inputWithoutVideoUnit: number
  singlePrice?: boolean
}

export type ChannelPriceForm = {
  channelId: number
  provider: string
  model: string
  referenceProvider: string
  referenceModel: string
  modelCategory: 'text' | 'image' | 'video' | 'audio'
  audioTranscriptionMode: 'file' | 'realtime' | null
  billingMeter: BillingMeter | null
  videoBillingMode: VideoBillingMode | null
  videoPriceTiers: ChannelVideoPriceTierForm[]
  inputPerMillion: number
  outputPerMillion: number
  cacheReadPerMillion: number
  cacheWritePerMillion: number | null
  unitPrice: number
  enabled: boolean
  hasPrice: boolean
  hasPriceRecord: boolean
  billingMeterLocked: boolean
  videoBillingModeLocked: boolean
  canUseImageBilling: boolean
  canUseVideoBilling: boolean
}

const open = defineModel<boolean>('open', { required: true })

const props = defineProps<{
  forms: Record<string, ChannelPriceForm>
  saving: boolean
  hasReferencePrice: (form: ChannelPriceForm) => boolean
  referencePriceSummary: (form: ChannelPriceForm) => string
  videoTierReferencePriceSummary: (
    form: ChannelPriceForm,
    tier: ChannelVideoPriceTierForm
  ) => string
  referencePriceFallbackLabel: (form: ChannelPriceForm) => string
}>()

const emit = defineEmits<{
  applyReferencePrices: []
  save: []
}>()

const { t } = useLocale()
const { billingCurrency } = useBillingCurrency()
const activePriceTab = ref('text')

const priceFormList = computed(() => Object.values(props.forms))

const textPriceForms = computed(() =>
  priceFormList.value.filter((form) => form.modelCategory === 'text')
)

const imagePriceForms = computed(() =>
  priceFormList.value.filter((form) => form.modelCategory === 'image')
)

const audioPriceForms = computed(() =>
  priceFormList.value.filter((form) => form.modelCategory === 'audio')
)

const videoPriceForms = computed(() =>
  priceFormList.value.filter((form) => form.modelCategory === 'video')
)

const standardPriceSections = computed(() =>
  [
    { key: 'text', title: t('textModelPrices'), forms: textPriceForms.value },
    { key: 'image', title: t('imageModelPrices'), forms: imagePriceForms.value },
    { key: 'audio', title: t('audioModelPrices'), forms: audioPriceForms.value }
  ].filter((section) => section.forms.length > 0)
)

const priceTabKeys = computed(() => [
  ...standardPriceSections.value.map((section) => section.key),
  ...(videoPriceForms.value.length > 0 ? ['video'] : [])
])

watch(
  [open, priceTabKeys],
  () => {
    if (priceTabKeys.value.includes(activePriceTab.value)) return
    activePriceTab.value = priceTabKeys.value[0] ?? 'text'
  },
  { immediate: true }
)

function formatCurrencyInput(value: number | string) {
  if (value === '' || value === undefined || value === null) return ''
  if (typeof value === 'number' && Number.isFinite(value)) {
    return `${billingCurrency.value === 'CNY' ? '¥' : '$'}${value.toFixed(2)}`
  }
  return `${billingCurrency.value === 'CNY' ? '¥' : '$'}${value}`
}

function parseCurrencyInput(value: string) {
  return value.replace(/^[¥$]/, '')
}

function billingMeterOptionLabel(row: ChannelPriceForm, billingMeter: BillingMeter) {
  if (row.canUseImageBilling && billingMeter === 'token') return t('pricePerMillionTokens')
  if (row.canUseImageBilling && billingMeter === 'image') return t('billingMeterPerCall')
  if (billingMeter === 'image') return t('billingMeterImageGeneration')
  if (billingMeter === 'video') return t('billingMeterVideo')
  if (billingMeter === 'audio') return t('videoBillingPerSecond')
  return t('billingMeterToken')
}

function billingMeterLabel(row: ChannelPriceForm) {
  return row.billingMeter
    ? billingMeterOptionLabel(row, row.billingMeter)
    : t('billingMeterRequired')
}

function inferredPerSecondPrice(
  pricePerMillionTokens: number,
  estimatedTokensPerSecond: number,
  resolutionsText: string,
  hasVideoInput = false
) {
  if (!Number.isFinite(pricePerMillionTokens) || pricePerMillionTokens <= 0) return 0
  const tokensPerSecond = resolvedVideoTokensPerSecondEstimate(
    estimatedTokensPerSecond,
    resolutionsText
  )
  const inputOutputDurationMultiplier = hasVideoInput ? 2 : 1
  const pricePerSecond =
    (pricePerMillionTokens * tokensPerSecond * inputOutputDurationMultiplier) / 1_000_000
  return Math.round(pricePerSecond * 100) / 100
}

function shouldInferPerSecondPrice(currentPrice: number, pricePerMillionTokens: number) {
  return (
    !Number.isFinite(currentPrice) ||
    currentPrice <= 0 ||
    Math.abs(currentPrice - pricePerMillionTokens) < 0.000001
  )
}

function inferPerSecondVideoPrices(row: ChannelPriceForm) {
  for (const tier of row.videoPriceTiers) {
    if (shouldInferPerSecondPrice(tier.inputWithoutVideoUnit, tier.inputWithoutVideo)) {
      tier.inputWithoutVideoUnit = inferredPerSecondPrice(
        tier.inputWithoutVideo,
        tier.estimatedTokensPerSecond,
        tier.resolutionsText,
        false
      )
    }
    if (shouldInferPerSecondPrice(tier.inputWithVideoUnit, tier.inputWithVideo)) {
      tier.inputWithVideoUnit = inferredPerSecondPrice(
        tier.inputWithVideo,
        tier.estimatedTokensPerSecond,
        tier.resolutionsText,
        true
      )
    }
  }
}

function applyVideoBillingMode(row: ChannelPriceForm) {
  row.billingMeter = 'video'
  if (row.videoBillingMode === 'per_second') {
    inferPerSecondVideoPrices(row)
  }
}

function videoBillingModeOptions() {
  return [
    { label: t('pricePerMillionTokens'), value: 'official_token' },
    { label: t('videoBillingPerSecond'), value: 'per_second' }
  ]
}

function videoBillingModeLabel(value: VideoBillingMode | null) {
  if (value === 'per_second') return t('videoBillingPerSecond')
  return t('pricePerMillionTokens')
}

function videoTierResolutionsLabel(tier: ChannelVideoPriceTierForm) {
  if (tier.resolutionLabel) return tier.resolutionLabel
  return (
    tier.resolutionsText
      .split(',')
      .map((resolution) => resolution.trim())
      .filter(Boolean)
      .join(', ') || '-'
  )
}

function videoTierPrimaryPrice(row: ChannelPriceForm, tier: ChannelVideoPriceTierForm) {
  return row.videoBillingMode === 'official_token'
    ? tier.inputWithoutVideo
    : tier.inputWithoutVideoUnit
}

function videoTierSecondaryPrice(row: ChannelPriceForm, tier: ChannelVideoPriceTierForm) {
  return row.videoBillingMode === 'official_token' ? tier.inputWithVideo : tier.inputWithVideoUnit
}

function updateVideoTierPrimaryPrice(
  row: ChannelPriceForm,
  tier: ChannelVideoPriceTierForm,
  value: number | undefined
) {
  if (row.videoBillingMode === 'official_token') {
    tier.inputWithoutVideo = value ?? 0
    return
  }
  tier.inputWithoutVideoUnit = value ?? 0
}

function updateVideoTierSecondaryPrice(
  row: ChannelPriceForm,
  tier: ChannelVideoPriceTierForm,
  value: number | undefined
) {
  if (row.videoBillingMode === 'official_token') {
    tier.inputWithVideo = value ?? 0
    return
  }
  tier.inputWithVideoUnit = value ?? 0
}
</script>

<template>
  <el-dialog
    v-model="open"
    class="channel-dialog price-dialog"
    :close-on-click-modal="false"
    :title="t('modelPriceDialogTitle')"
    width="min(940px, calc(100vw - 32px))"
  >
    <div class="price-editor-sections">
      <el-tabs v-if="priceTabKeys.length" v-model="activePriceTab" class="price-model-tabs">
        <el-tab-pane
          v-for="section in standardPriceSections"
          :key="section.key"
          :name="section.key"
        >
          <template #label>
            <span class="price-tab-label">
              {{ section.title }}
              <span>{{ section.forms.length }}</span>
            </span>
          </template>
          <div
            class="price-editor"
            :class="{ 'is-image-editor': section.key === 'image' || section.key === 'audio' }"
          >
            <div class="price-editor-head">
              <span>{{ t('model') }}</span>
              <span>{{ t('billingMeter') }}</span>
              <template v-if="section.key === 'image' || section.key === 'audio'">
                <span>{{ t('prices') }}</span>
              </template>
              <template v-else>
                <span>{{ t('inputOutputPriceShort') }}</span>
                <span>{{ t('cacheReadWritePriceShort') }}</span>
              </template>
              <span>{{ t('officialReferencePrice') }}</span>
            </div>

            <div class="price-editor-body">
              <div
                v-for="row in section.forms"
                :key="`${row.provider}:${row.model}`"
                class="price-editor-row"
              >
                <div class="price-model-cell" :title="row.model">
                  <span>{{ row.model }}</span>
                  <el-tag
                    v-if="row.audioTranscriptionMode"
                    class="audio-transcription-mode-tag"
                    effect="plain"
                    size="small"
                    :type="row.audioTranscriptionMode === 'realtime' ? 'success' : 'info'"
                  >
                    {{
                      row.audioTranscriptionMode === 'realtime'
                        ? t('realtimeAudioTranscription')
                        : t('fileAudioTranscription')
                    }}
                  </el-tag>
                </div>
                <div class="price-meter-cell">
                  <span v-if="row.billingMeterLocked" class="price-meter-static">
                    {{ billingMeterLabel(row) }}
                  </span>
                  <el-select
                    v-else
                    v-model="row.billingMeter"
                    class="price-meter-select"
                    popper-class="price-meter-select-dropdown"
                    :placeholder="t('billingMeterRequired')"
                  >
                    <el-option :label="billingMeterOptionLabel(row, 'token')" value="token" />
                    <el-option
                      v-if="row.canUseImageBilling"
                      :label="billingMeterOptionLabel(row, 'image')"
                      value="image"
                    />
                  </el-select>
                </div>
                <div
                  v-if="section.key === 'image' || section.key === 'audio'"
                  class="image-price-cell"
                >
                  <template v-if="row.billingMeter === 'token'">
                    <div class="image-price-group">
                      <span class="image-price-group-label">{{ t('inputOutputPairShort') }}</span>
                      <div class="price-pair-input">
                        <el-input-number
                          v-model="row.inputPerMillion"
                          class="price-number-input"
                          :controls="false"
                          :formatter="formatCurrencyInput"
                          :min="0"
                          :parser="parseCurrencyInput"
                          :step="0.01"
                        />
                        <span class="price-pair-separator">/</span>
                        <el-input-number
                          v-model="row.outputPerMillion"
                          class="price-number-input"
                          :controls="false"
                          :formatter="formatCurrencyInput"
                          :min="0"
                          :parser="parseCurrencyInput"
                          :step="0.01"
                        />
                      </div>
                    </div>
                    <div class="image-price-group">
                      <span class="image-price-group-label">{{
                        t('cacheReadWritePairShort')
                      }}</span>
                      <div class="price-pair-input">
                        <el-input-number
                          v-model="row.cacheReadPerMillion"
                          class="price-number-input"
                          :controls="false"
                          :formatter="formatCurrencyInput"
                          :min="0"
                          :parser="parseCurrencyInput"
                          :step="0.01"
                        />
                        <span class="price-pair-separator">/</span>
                        <el-input-number
                          v-model="row.cacheWritePerMillion"
                          class="price-number-input"
                          :controls="false"
                          :formatter="formatCurrencyInput"
                          :min="0"
                          :parser="parseCurrencyInput"
                          :step="0.01"
                        />
                      </div>
                    </div>
                  </template>
                  <div v-else-if="row.billingMeter === 'image'" class="image-price-group">
                    <div class="video-price-pair-input is-single">
                      <el-input-number
                        v-model="row.unitPrice"
                        class="video-tier-pair-number"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.01"
                      />
                    </div>
                  </div>
                  <div v-else-if="row.billingMeter === 'audio'" class="video-price-cell">
                    <div class="video-price-pair-input is-single">
                      <el-input-number
                        v-model="row.unitPrice"
                        class="video-tier-pair-number"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.0001"
                      />
                    </div>
                  </div>
                  <span v-else class="price-muted-cell">{{ t('billingMeterRequired') }}</span>
                </div>
                <template v-else>
                  <div class="price-pair-field">
                    <div v-if="row.billingMeter === 'token'" class="price-pair-input">
                      <el-input-number
                        v-model="row.inputPerMillion"
                        class="price-number-input"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.01"
                      />
                      <span class="price-pair-separator">/</span>
                      <el-input-number
                        v-model="row.outputPerMillion"
                        class="price-number-input"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.01"
                      />
                    </div>
                    <div v-else-if="row.billingMeter === 'image'" class="price-single-input">
                      <el-input-number
                        v-model="row.unitPrice"
                        class="price-number-input"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.01"
                      />
                      <span class="price-unit-label">{{ t('perImage') }}</span>
                    </div>
                    <span v-else class="price-muted-cell">{{ t('billingMeterRequired') }}</span>
                  </div>
                  <div class="price-pair-field">
                    <div v-if="row.billingMeter === 'token'" class="price-pair-input">
                      <el-input-number
                        v-model="row.cacheReadPerMillion"
                        class="price-number-input"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.01"
                      />
                      <span class="price-pair-separator">/</span>
                      <el-input-number
                        v-model="row.cacheWritePerMillion"
                        class="price-number-input"
                        :controls="false"
                        :formatter="formatCurrencyInput"
                        :min="0"
                        :parser="parseCurrencyInput"
                        :step="0.01"
                      />
                    </div>
                    <span v-else class="price-muted-cell">-</span>
                  </div>
                </template>
                <div class="reference-price-cell">
                  <template v-if="hasReferencePrice(row)">
                    <span class="reference-price-summary">{{ referencePriceSummary(row) }}</span>
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
        </el-tab-pane>

        <el-tab-pane v-if="videoPriceForms.length" name="video">
          <template #label>
            <span class="price-tab-label">
              {{ t('videoModelPrices') }}
              <span>{{ videoPriceForms.length }}</span>
            </span>
          </template>
          <div class="video-price-editor">
            <div class="video-price-head">
              <span>{{ t('model') }}</span>
              <span>{{ t('billingMeter') }}</span>
              <span class="video-spec-head">{{ t('videoTierResolutions') }}</span>
              <span>{{ t('videoTierPrice') }}</span>
              <span>{{ t('officialReferencePrice') }}</span>
            </div>

            <div class="video-price-body">
              <div
                v-for="row in videoPriceForms"
                :key="`${row.provider}:${row.model}`"
                class="video-price-model-row"
              >
                <div class="price-model-cell" :title="row.model">
                  <span>{{ row.model }}</span>
                </div>
                <div class="video-meter-cell">
                  <span v-if="row.videoBillingModeLocked" class="price-meter-static">
                    {{ videoBillingModeLabel(row.videoBillingMode) }}
                  </span>
                  <el-select
                    v-else
                    v-model="row.videoBillingMode"
                    class="price-meter-select video-mode-select"
                    popper-class="price-meter-select-dropdown"
                    :placeholder="t('videoBillingMode')"
                    @change="applyVideoBillingMode(row)"
                  >
                    <el-option
                      v-for="option in videoBillingModeOptions()"
                      :key="option.value"
                      :label="option.label"
                      :value="option.value"
                    />
                  </el-select>
                </div>
                <div class="video-tier-stack">
                  <div v-if="row.videoPriceTiers.length === 0" class="video-tier-row">
                    <span class="price-muted-cell">-</span>
                    <span class="price-muted-cell">{{ t('noKnownVideoTiers') }}</span>
                    <div class="reference-price-cell">
                      <el-tag
                        class="reference-price-fallback-tag"
                        :type="row.hasPrice ? 'info' : 'warning'"
                      >
                        {{ referencePriceFallbackLabel(row) }}
                      </el-tag>
                    </div>
                  </div>

                  <div
                    v-for="(tier, tierIndex) in row.videoPriceTiers"
                    :key="`${row.provider}:${row.model}:${tierIndex}`"
                    class="video-tier-row"
                  >
                    <span class="video-resolution-cell">{{ videoTierResolutionsLabel(tier) }}</span>
                    <div class="video-price-cell">
                      <div v-if="!tier.singlePrice" class="video-price-pair-labels">
                        <span>{{ tier.pricePairLeftLabel ?? t('videoInputWithoutVideo') }}</span>
                        <span>/</span>
                        <span>{{ tier.pricePairRightLabel ?? t('videoInputWithVideo') }}</span>
                      </div>
                      <div
                        class="video-price-pair-input"
                        :class="{ 'is-single': tier.singlePrice }"
                      >
                        <el-input-number
                          :model-value="videoTierPrimaryPrice(row, tier)"
                          class="video-tier-pair-number"
                          :controls="false"
                          :formatter="formatCurrencyInput"
                          :min="0"
                          :parser="parseCurrencyInput"
                          :step="0.01"
                          @update:model-value="updateVideoTierPrimaryPrice(row, tier, $event)"
                        />
                        <template v-if="!tier.singlePrice">
                          <span class="price-pair-separator">/</span>
                          <el-input-number
                            :model-value="videoTierSecondaryPrice(row, tier)"
                            class="video-tier-pair-number"
                            :controls="false"
                            :formatter="formatCurrencyInput"
                            :min="0"
                            :parser="parseCurrencyInput"
                            :step="0.01"
                            @update:model-value="updateVideoTierSecondaryPrice(row, tier, $event)"
                          />
                        </template>
                      </div>
                    </div>
                    <div class="reference-price-cell">
                      <template v-if="videoTierReferencePriceSummary(row, tier)">
                        <span class="reference-price-summary">
                          {{ videoTierReferencePriceSummary(row, tier) }}
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
            </div>
          </div>
        </el-tab-pane>
      </el-tabs>
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
.price-editor-sections {
  --price-model-column: clamp(220px, 24vw, 280px);
  --price-control-height: 30px;
  --price-primary-text: #1e293b;
  --price-secondary-text: #526174;
  --price-tertiary-text: #718096;
  --price-muted-text: #8a97a8;

  display: grid;
  gap: 16px;
  overflow: visible;
}

.price-model-tabs :deep(.el-tabs__header) {
  margin: 0 0 12px;
}

.price-model-tabs :deep(.el-tabs__nav-wrap::after) {
  background-color: #e2e8f0;
  height: 1px;
}

.price-tab-label {
  align-items: center;
  display: inline-flex;
  gap: 7px;
}

.price-tab-label span {
  align-items: center;
  background: #f3f7fb;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: var(--price-secondary-text);
  display: inline-flex;
  font-size: 11px;
  font-weight: 500;
  justify-content: center;
  line-height: 1;
  min-width: 26px;
  padding: 3px 8px;
}

.price-section {
  display: grid;
  gap: 8px;
}

.price-section-header {
  align-items: center;
  display: flex;
  justify-content: flex-start;
  min-width: 0;
}

.price-section-header h3 {
  align-items: center;
  color: var(--price-primary-text);
  display: inline-flex;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.2;
  margin: 0;
}

.price-section-header h3 span {
  align-items: center;
  background: #f3f7fb;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: var(--price-secondary-text);
  display: inline-flex;
  font-size: 11px;
  font-weight: 500;
  justify-content: center;
  min-width: 28px;
  padding: 3px 9px;
}

.price-editor {
  border: 1px solid #dfe6ef;
  border-radius: 7px;
  overflow: hidden;
}

.video-price-editor {
  border: 1px solid #dfe6ef;
  border-radius: 7px;
  overflow: hidden;
}

.price-editor-head,
.price-editor-row {
  align-items: center;
  display: grid;
  grid-template-columns:
    var(--price-model-column)
    106px
    144px
    176px
    minmax(132px, 0.9fr);
}

.price-editor.is-image-editor .price-editor-head,
.price-editor.is-image-editor .price-editor-row {
  grid-template-columns:
    var(--price-model-column)
    112px
    minmax(260px, 1fr)
    minmax(132px, 0.7fr);
}

.video-price-head {
  align-items: center;
  display: grid;
  grid-template-columns:
    var(--price-model-column)
    118px
    136px
    156px
    170px;
}

.price-editor-head,
.video-price-head {
  background: #f8fafc;
  border-bottom: 1px solid #e2e8f0;
  color: var(--price-secondary-text);
  font-size: 11px;
  font-weight: 500;
  line-height: 1.3;
  min-height: 42px;
}

.price-editor-head > span,
.price-editor-row > *,
.video-price-head > span,
.video-price-model-row > .price-model-cell,
.video-price-model-row > .video-meter-cell,
.video-tier-row > * {
  min-width: 0;
  padding: 0 8px;
}

.video-spec-head {
  text-align: center;
}

.video-price-head > span:nth-child(4) {
  text-align: center;
}

.price-editor-head > span:nth-child(2),
.video-price-head > span:nth-child(2) {
  text-align: center;
}

.price-editor.is-image-editor .price-editor-head > span:nth-child(3) {
  text-align: center;
}

.price-editor-row,
.video-price-model-row {
  background: #ffffff;
  min-height: 56px;
}

.video-price-model-row {
  align-items: stretch;
  display: grid;
  grid-template-columns:
    var(--price-model-column)
    118px
    minmax(0, 1fr);
  min-height: 60px;
}

.price-editor-row + .price-editor-row,
.video-price-model-row + .video-price-model-row {
  border-top: 1px solid #edf2f7;
}

.price-editor-row:nth-child(odd),
.video-price-model-row:nth-child(odd) {
  background: #fbfdff;
}

.price-model-cell {
  align-items: center;
  color: var(--price-primary-text);
  display: flex;
  font-size: 13px;
  font-weight: 560;
  gap: 7px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.price-model-cell span {
  overflow: hidden;
  text-overflow: ellipsis;
}

.price-model-cell .audio-transcription-mode-tag {
  flex: 0 0 auto;
  font-size: 10px;
  font-weight: 500;
  overflow: visible;
}

.price-pair-field {
  display: flex;
  justify-content: flex-start;
  min-width: 0;
}

.image-price-cell {
  align-items: center;
  display: flex;
  gap: 14px;
  justify-content: center;
  min-width: 0;
}

.image-price-group {
  align-items: center;
  display: grid;
  gap: 3px;
  justify-items: center;
  min-width: 0;
}

.image-price-group-label {
  color: var(--price-tertiary-text);
  font-size: 10px;
  font-weight: 400;
  line-height: 1.1;
  white-space: nowrap;
}

.price-meter-cell {
  align-items: center;
  display: flex;
  justify-content: center;
  min-width: 0;
}

.price-meter-select {
  width: 102px;
}

.price-meter-select :deep(.el-select__wrapper) {
  font-size: 12px;
  height: var(--price-control-height);
  min-height: var(--price-control-height);
  padding-left: 8px;
  padding-right: 6px;
}

.video-mode-select {
  width: 108px;
}

.price-meter-select :deep(.el-select__placeholder),
.price-meter-select :deep(.el-select__selected-item),
.video-mode-select :deep(.el-select__placeholder),
.video-mode-select :deep(.el-select__selected-item) {
  font-size: 12px;
  font-weight: 400;
}

:global(.price-meter-select-dropdown .el-select-dropdown__item) {
  font-size: 12px;
  height: 30px;
  line-height: 30px;
  padding: 0 10px;
}

.price-meter-static {
  align-items: center;
  background: #f5f7fb;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: var(--price-secondary-text);
  display: inline-flex;
  font-size: 12px;
  font-weight: 400;
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
  height: var(--price-control-height);
  min-height: var(--price-control-height);
  padding: 0 5px;
  width: 116px;
}

.price-single-input {
  width: 122px;
}

.price-muted-cell,
.price-unit-label {
  color: var(--price-muted-text);
  font-size: 11px;
  font-weight: 400;
  white-space: nowrap;
}

.price-number-input {
  background: transparent;
  border-radius: 0;
  box-shadow: none !important;
  flex: 0 1 47px;
  height: var(--price-control-height);
  line-height: var(--price-control-height);
  min-width: 0;
  width: 47px;
}

.price-number-input :deep(.el-input) {
  --el-input-height: var(--price-control-height);

  height: var(--price-control-height);
  line-height: var(--price-control-height);
}

.price-pair-separator {
  color: var(--price-muted-text);
  flex: 0 0 auto;
  font-size: 15px;
  font-weight: 400;
  line-height: 1;
}

.price-number-input :deep(.el-input__wrapper),
.price-number-input :deep(.el-input__wrapper:hover),
.price-number-input :deep(.el-input__wrapper.is-focus) {
  background: transparent;
  border-radius: 0;
  box-shadow: none !important;
  height: var(--price-control-height);
  min-height: var(--price-control-height);
  padding: 0;
}

.price-number-input :deep(.el-input__inner) {
  color: var(--price-primary-text);
  font-size: 13px;
  font-weight: 500;
  text-align: right;
}

.reference-price-cell {
  align-items: flex-start;
  color: var(--price-tertiary-text);
  display: grid;
  gap: 1px;
  line-height: 1.22;
}

.reference-price-summary {
  color: var(--price-tertiary-text);
  font-size: 10.5px;
  font-weight: 500;
  line-height: 1.45;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: pre-line;
}

.reference-price-fallback-tag {
  animation: none;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 400;
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

.video-meter-cell {
  align-items: center;
  display: flex;
  justify-content: center;
  min-width: 0;
}

.video-tier-stack {
  display: grid;
  min-width: 0;
}

.video-tier-row {
  align-items: center;
  display: grid;
  grid-template-columns:
    136px
    156px
    170px;
  min-height: 60px;
}

.video-tier-row + .video-tier-row {
  border-top: 1px solid #edf2f7;
}

.video-resolution-cell {
  color: var(--price-secondary-text);
  font-size: 12px;
  font-weight: 500;
  line-height: 1.25;
  overflow: hidden;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.video-tier-row > .price-muted-cell:first-child {
  text-align: center;
}

.video-price-cell {
  align-items: start;
  display: grid;
  gap: 2px;
  justify-items: center;
  min-width: 0;
}

.video-price-pair-labels {
  align-items: end;
  color: var(--price-tertiary-text);
  display: inline-grid;
  font-size: 10px;
  font-weight: 400;
  gap: 4px;
  grid-template-columns: auto auto auto;
  justify-content: center;
  line-height: 1.1;
  max-width: 100%;
  min-width: 118px;
  width: max-content;
}

.video-price-pair-labels span {
  white-space: nowrap;
}

.video-price-pair-labels span:nth-child(2) {
  color: var(--price-muted-text);
  font-weight: 500;
}

.video-price-pair-input {
  align-items: center;
  background: #ffffff;
  border: 1px solid #d8e0ec;
  border-radius: 6px;
  display: flex;
  gap: 3px;
  height: var(--price-control-height);
  min-height: var(--price-control-height);
  padding: 0 5px;
  width: 118px;
}

.video-price-pair-input.is-single {
  width: 58px;
}

.video-price-pair-input.is-single .video-tier-pair-number {
  flex: 1 1 auto;
  width: 100%;
}

.video-tier-pair-number {
  background: transparent;
  border-radius: 0;
  box-shadow: none !important;
  flex: 0 1 48px;
  height: var(--price-control-height);
  line-height: var(--price-control-height);
  min-width: 0;
  width: 48px;
}

.video-tier-pair-number :deep(.el-input) {
  --el-input-height: var(--price-control-height);

  height: var(--price-control-height);
  line-height: var(--price-control-height);
}

.video-tier-pair-number :deep(.el-input__inner) {
  color: var(--price-primary-text);
  font-size: 13px;
  font-weight: 500;
  text-align: right;
}

.video-tier-pair-number :deep(.el-input__wrapper),
.video-tier-pair-number :deep(.el-input__wrapper:hover),
.video-tier-pair-number :deep(.el-input__wrapper.is-focus) {
  background: transparent;
  border-radius: 0;
  box-shadow: none !important;
  height: var(--price-control-height);
  min-height: var(--price-control-height);
  padding: 0;
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
  font-weight: 650;
  line-height: 1.2;
}

:global(.price-dialog .el-dialog__body) {
  overflow: visible;
  padding: 18px 22px;
}

:global(.price-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 22px 18px;
}

:global(.price-dialog .dialog-footer .el-button) {
  border-radius: 7px;
  font-weight: 500;
  min-height: 34px;
  min-width: 70px;
}

@media (max-width: 760px) {
  .price-editor-sections {
    overflow: visible;
  }

  .price-editor-head,
  .video-price-head {
    display: none;
  }

  .price-editor,
  .video-price-editor {
    border-radius: 8px;
  }

  .price-editor-row,
  .video-price-model-row {
    align-items: stretch;
    gap: 10px;
    grid-template-columns: 1fr;
    padding: 14px;
  }

  .price-editor-row > *,
  .video-price-model-row > *,
  .video-tier-row > * {
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

  .video-price-cell {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .image-price-cell {
    align-items: stretch;
    display: grid;
    gap: 8px;
    justify-content: stretch;
  }

  .image-price-group {
    justify-items: stretch;
  }

  .image-price-group-label {
    text-align: left;
  }

  .video-tier-row {
    gap: 8px;
    grid-template-columns: 1fr;
    min-height: 0;
    padding: 10px 0;
  }

  .video-tier-row:first-child {
    padding-top: 0;
  }

  .video-tier-row:last-child {
    padding-bottom: 0;
  }

  .video-mode-select,
  .video-price-pair-labels,
  .price-pair-input,
  .price-single-input,
  .video-price-pair-input {
    width: 100%;
  }
}
</style>
