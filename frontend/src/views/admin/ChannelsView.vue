<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Delete,
  Edit,
  Lightning,
  Plus,
  Search,
  VideoPause,
  WarningFilled
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  getModelReferenceCatalog,
  getProviderModels,
  getPricingTemplates,
  getProviderPrices,
  syncPricingTemplates,
  upsertProviderPrice
} from '../../api/prices'
import { updateChannelModel, updateChannel } from '../../api/channels'
import { getAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import ChannelFormDialog from '../../components/admin/channels/ChannelFormDialog.vue'
import ChannelDiagnosticDialog from '../../components/admin/channels/ChannelDiagnosticDialog.vue'
import ChannelExpandPanel from '../../components/admin/channels/ChannelExpandPanel.vue'
import ChannelProbeTrendCell from '../../components/admin/channels/ChannelProbeTrendCell.vue'
import ChannelPriceDialog, {
  type ChannelPriceForm,
  type ChannelVideoPriceTierForm
} from '../../components/admin/channels/ChannelPriceDialog.vue'
import ModelPickerDialog from '../../components/admin/channels/ModelPickerDialog.vue'
import ProviderIcon from '../../components/common/ProviderIcon.vue'
import { useChannelDiagnostics } from '../../composables/useChannelDiagnostics'
import { useChannels } from '../../composables/useChannels'
import { useBillingCurrency } from '../../composables/useBillingCurrency'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type {
  BillingMeter,
  Channel,
  ChannelKey,
  ChannelModel,
  ModelReferenceCatalogRecord,
  PricingTemplate,
  ProviderModel,
  ProviderPrice,
  VideoBillingMode,
  VideoPriceTier
} from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { splitCommaList } from '../../utils/channel'
import {
  derivedCacheReadPrice,
  findPricingTemplate,
  isProviderPriceConfigured,
  priceKey,
  pricingReferenceModelAliases,
  resolvedVideoTokensPerSecondEstimate
} from '../../utils/pricing'

const { locale, t } = useLocale()
const {
  billingCurrency,
  currencySymbol,
  formatPricePerMillion,
  majorToMicroAmount,
  microAmountToMajor
} = useBillingCurrency()

defineOptions({
  name: 'ChannelsView'
})

const {
  channels,
  channelKeys,
  providerOptions,
  keyCounts,
  loading,
  createDialogOpen,
  editDialogOpen,
  modelPickerDialogOpen,
  creating,
  fetchingModels,
  updating,
  deletingId,
  deletingKeyId,
  copyingKeyId,
  editingChannel,
  editingChannelKeys,
  createForm,
  editForm,
  createBaseUrl,
  editBaseUrl,
  secretInput,
  editSecretInput,
  fetchedModels,
  selectedFetchedModels,
  allFetchedModelsSelected,
  modelsInputPlaceholder,
  modelsInputReadonly,
  selectCreateProvider,
  openCreateDialog,
  openEditDialog,
  fetchCreateModels,
  fetchEditModels,
  toggleAllFetchedModels,
  loadChannels,
  submitChannel: submitChannelBase,
  submitEditChannel: submitEditChannelBase,
  confirmDeleteChannelKey,
  copyChannelKeySecret,
  confirmDeleteChannel
} = useChannels(t)

const prices = ref<ProviderPrice[]>([])
const templates = ref<PricingTemplate[]>([])
const providerModels = ref<ProviderModel[]>([])
const modelReferenceCatalog = ref<ModelReferenceCatalogRecord[]>([])
const bundledVideoTierCatalog = ref<BundledVideoTierRecord[]>([])
const pricingLoading = ref(true)
const servicePolicy = ref<ServicePolicy | null>(null)
const channelsLoaded = ref(false)
const priceDialogOpen = ref(false)
const savingPrices = ref(false)
const channelTableRef = ref()
const togglingRuntimeKeys = useReactiveSet<string>()
const togglingChannelIds = useReactiveSet<number>()
const channelSearch = ref('')
const appliedChannelSearch = ref('')
const channelStatusFilter = ref<'all' | 'normal' | 'attention' | 'disabled'>('all')
const channelCurrentPage = ref(1)
const channelPageSize = ref(20)
const channelPageSizes = [20, 50, 100]
const priceForms = reactive<Record<string, ChannelPriceForm>>({})
const anyVideoTierResolution = '*'

type BundledPricingModel = {
  cost?: {
    video_tiers?: unknown
  }
}

type BundledPricingProvider = {
  models?: Record<string, BundledPricingModel>
}

type BundledPricingCatalog = Record<string, BundledPricingProvider>

type BundledVideoTierRecord = {
  provider: string
  model: string
  videoTiers: ReferenceVideoTier[]
}

const priceByModel = computed(
  () => new Map(prices.value.map((price) => [priceKey(price.provider, price.model), price]))
)
const isInternalServiceMode = computed(() => servicePolicy.value?.service_mode === 'internal')
const providerModelByModel = computed(
  () => new Map(providerModels.value.map((model) => [priceKey(model.provider, model.model), model]))
)

const diagnostic = useChannelDiagnostics(loadChannels)
const {
  inProgress: diagnosticInProgress,
  isChannelDiagnosing,
  run: runChannelDiagnostic
} = diagnostic

const filteredChannels = computed(() => {
  const keyword = appliedChannelSearch.value.trim().toLowerCase()
  return channels.value.filter((channel) => {
    const runtimeStatus = channelRuntimeStatus(channel)
    const matchesStatus =
      channelStatusFilter.value === 'all' || runtimeStatus === channelStatusFilter.value
    const matchesKeyword =
      !keyword ||
      [channel.name, channel.provider, ...channelModelList(channel)].some((value) =>
        value.toLowerCase().includes(keyword)
      )
    return matchesStatus && matchesKeyword
  })
})

const hasChannelSearchCriteria = computed(
  () => appliedChannelSearch.value.trim().length > 0 || channelStatusFilter.value !== 'all'
)

const paginatedChannels = computed(() => {
  const start = (channelCurrentPage.value - 1) * channelPageSize.value
  return filteredChannels.value.slice(start, start + channelPageSize.value)
})

const channelTotalPages = computed(() =>
  Math.ceil(filteredChannels.value.length / channelPageSize.value)
)

const channelHasPagination = computed(() => filteredChannels.value.length > channelPageSize.value)

const probeTrendLatencyScale = computed(() => {
  const latencies = channels.value.flatMap((channel) =>
    channel.probe_samples.map((sample) => sample.latency_ms ?? 0).filter((latency) => latency > 0)
  )
  return Math.max(...latencies, 1)
})

function handleChannelPageSizeChange(size: number) {
  channelPageSize.value = size
  channelCurrentPage.value = 1
}

function searchChannels() {
  appliedChannelSearch.value = channelSearch.value.trim()
  channelCurrentPage.value = 1
}

function clearChannelSearch() {
  channelSearch.value = ''
  appliedChannelSearch.value = ''
  channelCurrentPage.value = 1
}

function channelModelList(row: Channel) {
  const models = row.models?.length
    ? row.models.map((model) => model.model)
    : row.endpoints.flatMap((endpoint) => endpoint.models)
  return Array.from(new Set(models.map((model) => model.trim()).filter(Boolean)))
}

function channelModelRecords(row: Channel) {
  if (row.models?.length) return row.models
  return channelModelList(row).map(
    (model) =>
      ({
        id: 0,
        channel_id: row.id,
        provider: row.provider,
        model,
        enabled: true,
        status: 'available',
        runtime_status: 'normal',
        success_count: 0,
        failure_count: 0,
        billing_enabled: Boolean(priceByModel.value.get(priceKey(row.provider, model))?.enabled),
        price_configured: Boolean(priceByModel.value.get(priceKey(row.provider, model))),
        created_at: '',
        updated_at: ''
      }) as ChannelModel
  )
}

function modelStatusLabel(status: ChannelModel['status']) {
  const labels: Record<ChannelModel['status'], string> = {
    available: t('modelStatusAvailable'),
    missing: t('modelStatusMissing'),
    disabled: t('modelStatusDisabled')
  }
  return labels[status]
}

function runtimeStatusLabel(status: ChannelModel['runtime_status']) {
  const labels: Record<ChannelModel['runtime_status'], string> = {
    normal: t('modelStatusNormal'),
    cooldown: t('modelStatusCooldown'),
    failed: t('modelStatusFailed')
  }
  return labels[status]
}

function channelPriceStatus(row: Channel) {
  const models = channelModelRecords(row)
  if (models.length === 0) {
    return { missing: 0, total: 0, type: 'info' as const, label: '-' }
  }

  if (pricingLoading.value && prices.value.length === 0) {
    return { missing: 0, total: models.length, type: 'info' as const, label: '-' }
  }

  const missing = models.filter((model) => !model.billing_enabled).length
  if (missing === 0) {
    return { missing, total: models.length, type: 'success' as const, label: t('priceReady') }
  }

  if (missing === models.length) {
    return { missing, total: models.length, type: 'danger' as const, label: t('priceMissing') }
  }

  return {
    missing,
    total: models.length,
    type: 'warning' as const,
    label: `${t('pricePartialMissing')} ${missing}/${models.length}`
  }
}

function channelPriceRows(row: Channel) {
  return channelModelRecords(row).map((channelModel) => {
    const model = channelModel.model
    const price = priceByModel.value.get(priceKey(row.provider, model))
    const hasConfiguredPrice = channelModel.price_configured || isProviderPriceConfigured(price)
    if (pricingLoading.value && prices.value.length === 0) {
      return {
        model,
        disabled: false,
        missing: false,
        billingEnabled: false,
        runtimeStatus: channelModel.runtime_status,
        runtimeStatusLabel: runtimeStatusLabel(channelModel.runtime_status),
        runtimeToggleDisabled: true,
        runtimeEnabled: Boolean(channelModel.enabled),
        upstreamMissing: channelModel.status === 'missing',
        inputPrice: '-',
        outputPrice: '-',
        cacheReadPrice: '-',
        cacheWritePrice: '-',
        cachePrice: '-',
        price: '-'
      }
    }

    const inputMicros = channelModel.input_price_micros ?? price?.input_price_micros
    const outputMicros = channelModel.output_price_micros ?? price?.output_price_micros
    const billingMeter = channelModel.billing_meter ?? price?.billing_meter
    const unitMicros = channelModel.unit_price_micros ?? price?.unit_price_micros
    const cacheReadMicros =
      channelModel.cache_read_price_micros ??
      price?.cache_read_price_micros ??
      (inputMicros === undefined ? undefined : derivedCacheReadPrice(inputMicros))
    const cacheWriteMicros =
      channelModel.cache_write_price_micros ?? price?.cache_write_price_micros
    const billingEnabled = Boolean(channelModel.billing_enabled)
    const modelEnabled = Boolean(channelModel.enabled)
    const unitPrice =
      unitMicros !== undefined && unitMicros !== null
        ? formatPricePerMillion(unitMicros, 'en-US')
        : t('priceMissing')
    const inputPrice =
      inputMicros !== undefined && inputMicros !== null
        ? formatPricePerMillion(inputMicros, 'en-US')
        : t('priceMissing')
    const outputPrice =
      outputMicros !== undefined && outputMicros !== null
        ? formatPricePerMillion(outputMicros, 'en-US')
        : t('priceMissing')
    const cacheReadPrice =
      cacheReadMicros !== undefined && cacheReadMicros !== null
        ? formatPricePerMillion(cacheReadMicros, 'en-US')
        : t('priceMissing')
    const cacheWritePrice = hasConfiguredPrice
      ? cacheWriteMicros === undefined || cacheWriteMicros === null
        ? `${currencySymbol.value}0`
        : formatPricePerMillion(cacheWriteMicros as number, 'en-US')
      : t('priceMissing')
    const modelStatus = channelModel.status
    const upstreamMissing = modelStatus === 'missing'
    return {
      model,
      disabled: !modelEnabled,
      missing: !hasConfiguredPrice,
      billingEnabled,
      runtimeStatus: channelModel.runtime_status,
      runtimeStatusLabel: runtimeStatusLabel(channelModel.runtime_status),
      runtimeToggleDisabled: !billingEnabled || upstreamMissing || isRuntimeToggling(row.id, model),
      runtimeEnabled: modelEnabled,
      upstreamMissing,
      modelStatus,
      modelStatusLabel: modelStatusLabel(modelStatus),
      inputPrice,
      outputPrice,
      cacheReadPrice,
      cacheWritePrice,
      cachePrice:
        billingMeter === 'image' || billingMeter === 'video'
          ? '-'
          : price && billingMeter === 'token'
            ? `${cacheReadPrice} / ${cacheWritePrice}`
            : t('priceMissing'),
      price:
        billingMeter === 'image'
          ? `${unitPrice} / ${t('perImage')}`
          : billingMeter === 'video'
            ? t('billingBasisMultiTierVideo')
            : price && billingMeter === 'token'
              ? `${inputPrice} / ${outputPrice}`
              : t('priceMissing')
    }
  })
}

function channelPricePreviewRows(row: Channel) {
  return channelPriceRows(row).slice(0, 3)
}

function channelPriceOverflowCount(row: Channel) {
  return Math.max(channelModelList(row).length - channelPricePreviewRows(row).length, 0)
}

function runtimeKey(channelId: number, model: string) {
  return `${channelId}:${model}`
}

function isRuntimeToggling(channelId: number, model: string) {
  return togglingRuntimeKeys.has(runtimeKey(channelId, model))
}

function isChannelToggling(channelId: number) {
  return togglingChannelIds.has(channelId)
}

function channelRuntimeStatus(row: Channel) {
  if (!row.enabled) {
    return 'disabled'
  }
  return row.endpoints.length > 0 && row.endpoints.every((endpoint) => endpoint.healthy)
    ? 'normal'
    : 'attention'
}

function channelRowClassName({ row }: { row: Channel }) {
  return row.enabled ? '' : 'channel-row-is-disabled'
}

function toggleChannelRowExpansion(row: Channel) {
  channelTableRef.value?.toggleRowExpansion(row)
}
function handleChannelExpandChange(row: Channel, expandedRows: Channel[]) {
  if (!expandedRows.includes(row)) return
  for (const expandedRow of expandedRows) {
    if (expandedRow.id !== row.id) {
      channelTableRef.value?.toggleRowExpansion(expandedRow, false)
    }
  }
}

function channelCredentialSummary(row: Channel) {
  const keys = channelKeys.value.filter((key) => key.channel_id === row.id)
  const count = keyCounts.value.get(row.id) ?? 0
  const enabledCount = keys.filter((key) => key.enabled).length
  const disabledCount = keys.filter((key) => !key.enabled).length
  const healthyCount = keys.filter((key) => key.healthy).length
  const unhealthyCount = Math.max(keys.length - healthyCount, 0)
  const title = keyStatusTooltip(keys, {
    enabledCount,
    disabledCount,
    healthyCount,
    unhealthyCount
  })
  if (row.use_credentials) {
    return {
      label: t('credentialFiles'),
      title
    }
  }
  return {
    label: `${count} ${t('channelKeyUnit')}`,
    title
  }
}

function keyStatusTooltip(
  keys: ChannelKey[],
  counts: {
    enabledCount: number
    disabledCount: number
    healthyCount: number
    unhealthyCount: number
  }
) {
  if (keys.length === 0) return t('channelNoKeyHint')

  return [
    `${t('enabledKeyCount')}: ${counts.enabledCount}`,
    `${t('disabledKeyCount')}: ${counts.disabledCount}`,
    `${t('healthyKeyCount')}: ${counts.healthyCount}`,
    `${t('abnormalKeyCount')}: ${counts.unhealthyCount}`,
    t('channelKeyMaskedHint')
  ].join('\n')
}

async function loadPricingData() {
  await withLoading(pricingLoading, async () => {
    try {
      const [
        fetchedPrices,
        fetchedTemplates,
        fetchedProviderModels,
        fetchedModelReferenceCatalog,
        fetchedBundledVideoTierCatalog
      ] = await Promise.all([
        getProviderPrices(),
        getPricingTemplates(),
        getProviderModels(),
        getModelReferenceCatalog(),
        getBundledVideoTierCatalog()
      ])
      prices.value = fetchedPrices
      templates.value = fetchedTemplates
      providerModels.value = fetchedProviderModels
      modelReferenceCatalog.value = fetchedModelReferenceCatalog
      bundledVideoTierCatalog.value = fetchedBundledVideoTierCatalog
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

function capabilityValues(capabilities: Record<string, unknown>, path: string[]) {
  let current: unknown = capabilities
  for (const key of path) {
    if (!current || typeof current !== 'object') return []
    current = (current as Record<string, unknown>)[key]
  }
  return Array.isArray(current)
    ? current.map((value) => String(value).trim().toLowerCase()).filter(Boolean)
    : []
}

function modelOutputModalities(provider: string, model: string) {
  const catalogRecord = findReferenceCatalogRecord(provider, model)
  const catalogOutput = catalogRecord
    ? capabilityValues(catalogRecord.capabilities, ['modalities', 'output'])
    : []
  if (catalogOutput.length > 0) return catalogOutput

  const record = findReferenceProviderModel(provider, model)
  const output = record ? capabilityValues(record.capabilities, ['modalities', 'output']) : []
  if (output.length > 0) return output

  const template = findPricingTemplate(templates.value, provider, model)
  if (!template || template.provider === provider) return output

  const referenceRecord = findReferenceProviderModel(template.provider, model)
  return referenceRecord
    ? capabilityValues(referenceRecord.capabilities, ['modalities', 'output'])
    : output
}

function canUseImageBilling(provider: string, model: string) {
  const output = modelOutputModalities(provider, model)
  return output.length === 1 && output[0] === 'image'
}

function canUseSeedanceVideoBilling(provider: string, model: string) {
  return (
    canonicalReferenceProvider(provider) === 'doubao' && model.toLowerCase().includes('seedance')
  )
}

function defaultBillingMeterForModel(provider: string, model: string) {
  if (canUseSeedanceVideoBilling(provider, model)) return 'video'
  return canUseImageBilling(provider, model) ? 'image' : 'token'
}

function isBillingMeterLocked(provider: string, model: string) {
  return canUseSeedanceVideoBilling(provider, model) || !canUseImageBilling(provider, model)
}

function templateAppliesToForm(template: PricingTemplate, form: ChannelPriceForm) {
  return (
    template.billing_meter ===
    (form.billingMeter ?? defaultBillingMeterForModel(form.provider, form.model))
  )
}

function findApplicablePricingTemplate(form: ChannelPriceForm) {
  const template = findPricingTemplate(templates.value, form.provider, form.model)
  return template && templateAppliesToForm(template, form) ? template : undefined
}

function hasManualPriceInput(form: ChannelPriceForm) {
  if (form.canUseSeedanceVideoBilling && form.videoBillingMode) {
    return form.videoPriceTiers.some((tier) => {
      if (form.videoBillingMode === 'official_token') {
        return tier.inputWithVideo > 0 || tier.inputWithoutVideo > 0
      }
      return tier.inputWithVideoUnit > 0 || tier.inputWithoutVideoUnit > 0
    })
  }
  if (form.billingMeter === 'image') return form.unitPrice > 0
  return (
    form.inputPerMillion > 0 ||
    form.outputPerMillion > 0 ||
    form.cacheReadPerMillion > 0 ||
    (form.cacheWritePerMillion ?? 0) > 0
  )
}

function hasEnabledBillablePrice(price?: ProviderPrice, billingMeter?: BillingMeter | null) {
  if (!price?.enabled) return false
  if (billingMeter && price.billing_meter !== billingMeter) return false
  if (price.video_billing_mode) {
    return (price.video_price_tiers ?? []).length > 0
  }
  if (price.billing_meter === 'image') return (price.unit_price_micros ?? 0) > 0
  if (price.billing_meter === 'video') return (price.video_price_tiers ?? []).length > 0
  return price.input_price_micros > 0 || price.output_price_micros > 0
}

function shouldSavePriceForm(form: ChannelPriceForm) {
  return form.hasPriceRecord || hasReferencePrice(form) || hasManualPriceInput(form)
}

function shouldEnablePriceForm(form: ChannelPriceForm) {
  return form.enabled || hasManualPriceInput(form)
}

type ReferenceVideoTier = {
  resolution?: string
  label?: string
  tiers?: Record<string, number | null | undefined>
}

function canonicalReferenceProvider(provider: string) {
  const normalizedProvider = provider.trim().toLowerCase()
  if (
    normalizedProvider === 'doubao' ||
    normalizedProvider === 'volcengine' ||
    normalizedProvider === 'volcengine-ark'
  ) {
    return 'doubao'
  }
  return normalizedProvider
}

function providerMatchesReference(left: string, right: string) {
  return canonicalReferenceProvider(left) === canonicalReferenceProvider(right)
}

function isReferenceVideoTier(value: unknown): value is ReferenceVideoTier {
  if (!value || typeof value !== 'object') return false
  const tier = value as ReferenceVideoTier
  return Boolean(tier.tiers && typeof tier.tiers === 'object')
}

function bundledVideoTierRecordsFromPricingCatalog(catalog: BundledPricingCatalog) {
  return Object.entries(catalog).flatMap(([provider, providerData]) => {
    return Object.entries(providerData.models ?? {}).flatMap(([model, modelData]) => {
      const value = modelData.cost?.video_tiers
      const videoTiers = Array.isArray(value) ? value.filter(isReferenceVideoTier) : []
      if (videoTiers.length === 0) return []
      return [
        {
          provider: canonicalReferenceProvider(provider),
          model,
          videoTiers
        }
      ]
    })
  })
}

async function getBundledVideoTierCatalog() {
  try {
    const response = await fetch(`${import.meta.env.BASE_URL}model-pricing.json`)
    if (!response.ok) return []
    const catalog = (await response.json()) as BundledPricingCatalog
    return bundledVideoTierRecordsFromPricingCatalog(catalog)
  } catch {
    return []
  }
}

function referenceVideoTierResolutions(tier: ReferenceVideoTier) {
  const resolution = tier.resolution?.trim()
  return resolution ? splitCommaList(resolution) : [anyVideoTierResolution]
}

function referenceVideoTierResolutionLabel(tier: ReferenceVideoTier) {
  const label = referenceVideoTierDisplayLabel(tier)
  if (label) return label
  return tier.resolution?.trim() ? '' : t('videoTierAnyResolution')
}

function referenceVideoTierDisplayLabel(tier: ReferenceVideoTier) {
  const label = tier.label?.trim()
  if (!label) return ''
  if (billingCurrency.value !== 'CNY' || !isMainlandReferenceVideoTier(tier)) return label
  const strippedLabel = label
    .replace(/^(中国内地|中国大陆)\s*(?:[·・|/\\-]\s*)?/u, '')
    .replace(/^(mainland\s*china|china\s*mainland|chinese\s*mainland)\s*(?:[·・|/\\-]\s*)?/iu, '')
    .trim()
  return strippedLabel || tier.resolution?.trim() || label
}

function videoTierFormResolutionLabel(resolution: string) {
  return resolution.trim() === anyVideoTierResolution ? t('videoTierAnyResolution') : undefined
}

function referenceVideoTierHasAudio(tier: ReferenceVideoTier) {
  return tier.tiers?.with_audio != null || tier.tiers?.without_audio != null
}

function referenceVideoTierInputWithoutVideo(tier: ReferenceVideoTier) {
  return tier.tiers?.input_without_video ?? tier.tiers?.without_audio ?? 0
}

function referenceVideoTierInputWithVideo(tier: ReferenceVideoTier) {
  return tier.tiers?.input_with_video ?? tier.tiers?.with_audio ?? 0
}

function referenceVideoTierMatchesFormResolutions(
  tier: ReferenceVideoTier,
  formResolutions: Set<string>
) {
  return referenceVideoTierResolutions(tier).some((resolution) => {
    const normalizedResolution = resolution.trim().toLowerCase()
    return (
      normalizedResolution === anyVideoTierResolution ||
      formResolutions.has(anyVideoTierResolution) ||
      formResolutions.has(normalizedResolution)
    )
  })
}

function isAnyReferenceVideoTier(tier: ReferenceVideoTier) {
  return referenceVideoTierResolutions(tier).some(
    (resolution) => resolution.trim().toLowerCase() === anyVideoTierResolution
  )
}

function referenceVideoTierSummary(tier: ReferenceVideoTier) {
  if (referenceVideoTierHasAudio(tier)) {
    return `${t('videoTierWithAudio')} ${currencySymbol.value}${referenceVideoTierInputWithVideo(tier)}/${t('pricePerMillionTokens')}\n${t('videoTierWithoutAudio')} ${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}/${t('pricePerMillionTokens')}`
  }

  const leftLabel = referenceVideoTierHasAudio(tier)
    ? t('videoTierWithoutAudio')
    : t('videoInputWithoutVideo')
  const rightLabel = referenceVideoTierHasAudio(tier)
    ? t('videoTierWithAudio')
    : t('videoInputWithVideo')
  return `${leftLabel} ${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}\n${rightLabel} ${currencySymbol.value}${referenceVideoTierInputWithVideo(tier)}`
}

function seedanceReferenceVideoTiers(provider: string, model: string) {
  const catalogRecord = findReferenceCatalogRecord(provider, model)
  const catalogValue = catalogRecord?.capabilities?.video_tiers
  if (Array.isArray(catalogValue) && catalogValue.length > 0) {
    return filterReferenceVideoTiersForCurrency(catalogValue as ReferenceVideoTier[])
  }

  const record = findReferenceProviderModel(provider, model)
  const value = record?.capabilities?.video_tiers
  if (Array.isArray(value) && value.length > 0) {
    return filterReferenceVideoTiersForCurrency(value as ReferenceVideoTier[])
  }

  const bundledRecord = findBundledVideoTierRecord(provider, model)
  return filterReferenceVideoTiersForCurrency(bundledRecord?.videoTiers ?? [])
}

function isMainlandReferenceVideoTier(tier: ReferenceVideoTier) {
  const label = tier.label?.trim() ?? ''
  return (
    label.includes('中国内地') ||
    label.includes('中国大陆') ||
    /mainland\s*china|china\s*mainland|chinese\s*mainland/i.test(label)
  )
}

function filterReferenceVideoTiersForCurrency(tiers: ReferenceVideoTier[]) {
  if (billingCurrency.value !== 'CNY') return tiers
  const mainlandTiers = tiers.filter(isMainlandReferenceVideoTier)
  return mainlandTiers.length > 0 ? mainlandTiers : tiers
}

function findReferenceCatalogRecord(provider: string, model: string) {
  const aliases = pricingReferenceModelAliases(model)
  return modelReferenceCatalog.value.find((record) => {
    if (!record.enabled || !providerMatchesReference(record.provider, provider)) return false
    return [...pricingReferenceModelAliases(record.model)].some((alias) => aliases.has(alias))
  })
}

function findReferenceProviderModel(provider: string, model: string) {
  const exact = providerModelByModel.value.get(priceKey(provider, model))
  if (exact) return exact

  const aliases = pricingReferenceModelAliases(model)
  return providerModels.value.find((record) => {
    if (!providerMatchesReference(record.provider, provider)) return false
    return [...pricingReferenceModelAliases(record.model)].some((alias) => aliases.has(alias))
  })
}

function findBundledVideoTierRecord(provider: string, model: string) {
  const aliases = pricingReferenceModelAliases(model)
  return bundledVideoTierCatalog.value.find((record) => {
    if (!providerMatchesReference(record.provider, provider)) return false
    return [...pricingReferenceModelAliases(record.model)].some((alias) => aliases.has(alias))
  })
}

function videoPriceTierPairLabels(referenceTier?: ReferenceVideoTier) {
  if (!referenceTier || !referenceVideoTierHasAudio(referenceTier)) return {}
  return {
    pricePairLeftLabel: t('videoTierWithoutAudio'),
    pricePairRightLabel: t('videoTierWithAudio')
  }
}

function savedVideoTierMatchesReferenceTier(
  tier: VideoPriceTier,
  referenceTier: ReferenceVideoTier
) {
  const formResolutions = new Set(
    tier.resolutions.flatMap(splitCommaList).map((item) => item.toLowerCase())
  )
  return referenceVideoTierMatchesFormResolutions(referenceTier, formResolutions)
}

function videoPriceTierFormFromSavedTier(
  tier: VideoPriceTier | undefined,
  resolution: string,
  referenceTier?: ReferenceVideoTier
) {
  return {
    resolutionsText: resolution,
    resolutionLabel: referenceTier
      ? referenceVideoTierResolutionLabel(referenceTier)
      : videoTierFormResolutionLabel(resolution),
    ...videoPriceTierPairLabels(referenceTier),
    inputWithVideo:
      microAmountToMajor(tier?.input_with_video_micros ?? 0) ||
      (referenceTier ? referenceVideoTierInputWithVideo(referenceTier) : 0),
    inputWithoutVideo:
      microAmountToMajor(tier?.input_without_video_micros ?? 0) ||
      (referenceTier ? referenceVideoTierInputWithoutVideo(referenceTier) : 0),
    estimatedTokensPerSecond: resolvedVideoTokensPerSecondEstimate(
      referenceTier && isAnyReferenceVideoTier(referenceTier)
        ? null
        : tier?.estimated_tokens_per_second,
      resolution
    ),
    inputWithVideoUnit: microAmountToMajor(tier?.input_with_video_unit_micros ?? 0),
    inputWithoutVideoUnit: microAmountToMajor(tier?.input_without_video_unit_micros ?? 0)
  }
}

function videoPriceTiersToForm(
  tiers: VideoPriceTier[] = [],
  referenceTiers: ReferenceVideoTier[] = []
) {
  const anyReferenceTier = referenceTiers.find(isAnyReferenceVideoTier)
  if (anyReferenceTier) {
    const savedTier = tiers.find((tier) =>
      savedVideoTierMatchesReferenceTier(tier, anyReferenceTier)
    )
    return [
      videoPriceTierFormFromSavedTier(
        savedTier ?? tiers[0],
        anyVideoTierResolution,
        anyReferenceTier
      )
    ]
  }

  return tiers.flatMap((tier) =>
    tier.resolutions.flatMap(splitCommaList).map((resolution) => {
      const formResolutions = new Set(splitCommaList(resolution).map((item) => item.toLowerCase()))
      const referenceTier = referenceTiers.find((item) =>
        referenceVideoTierMatchesFormResolutions(item, formResolutions)
      )
      return videoPriceTierFormFromSavedTier(tier, resolution, referenceTier)
    })
  )
}

function referenceVideoTiersToForm(tiers: ReferenceVideoTier[]) {
  return tiers.flatMap((tier) =>
    referenceVideoTierResolutions(tier).map((resolution) => ({
      resolutionsText: resolution,
      resolutionLabel: referenceVideoTierResolutionLabel(tier),
      pricePairLeftLabel: referenceVideoTierHasAudio(tier) ? t('videoTierWithoutAudio') : undefined,
      pricePairRightLabel: referenceVideoTierHasAudio(tier) ? t('videoTierWithAudio') : undefined,
      inputWithVideo: referenceVideoTierInputWithVideo(tier),
      inputWithoutVideo: referenceVideoTierInputWithoutVideo(tier),
      estimatedTokensPerSecond: resolvedVideoTokensPerSecondEstimate(null, resolution),
      inputWithVideoUnit: 0,
      inputWithoutVideoUnit: 0
    }))
  )
}

function videoPriceTiersPayload(form: ChannelPriceForm): VideoPriceTier[] {
  return form.videoPriceTiers
    .map((tier) => ({
      resolutions: splitCommaList(tier.resolutionsText),
      input_with_video_micros:
        form.videoBillingMode === 'official_token' ? majorToMicroAmount(tier.inputWithVideo) : null,
      input_without_video_micros:
        form.videoBillingMode === 'official_token'
          ? majorToMicroAmount(tier.inputWithoutVideo)
          : null,
      estimated_tokens_per_second:
        form.videoBillingMode === 'official_token'
          ? Math.max(1, Math.round(tier.estimatedTokensPerSecond || 0))
          : null,
      input_with_video_unit_micros:
        form.videoBillingMode === 'per_second' ? majorToMicroAmount(tier.inputWithVideoUnit) : null,
      input_without_video_unit_micros:
        form.videoBillingMode === 'per_second'
          ? majorToMicroAmount(tier.inputWithoutVideoUnit)
          : null
    }))
    .filter((tier) => tier.resolutions.length > 0)
}

function representativeVideoPriceMicros(
  tiers: VideoPriceTier[],
  mode: VideoBillingMode | null,
  withVideo = false
) {
  const tier = tiers[0]
  if (!tier || !mode) return 0
  if (mode === 'official_token') {
    return (
      (withVideo ? tier.input_with_video_micros : tier.input_without_video_micros) ??
      tier.input_without_video_micros ??
      tier.input_with_video_micros ??
      0
    )
  }
  return (
    (withVideo ? tier.input_with_video_unit_micros : tier.input_without_video_unit_micros) ??
    tier.input_without_video_unit_micros ??
    tier.input_with_video_unit_micros ??
    0
  )
}

function openPriceDialog(row: Channel) {
  for (const key of Object.keys(priceForms)) {
    delete priceForms[key]
  }

  for (const model of channelModelList(row)) {
    const key = priceKey(row.provider, model)
    const price = priceByModel.value.get(key)
    const template = findPricingTemplate(templates.value, row.provider, model)
    const supportsImageBilling = canUseImageBilling(row.provider, model)
    const supportsSeedanceVideoBilling = canUseSeedanceVideoBilling(row.provider, model)
    const referenceVideoTiers = supportsSeedanceVideoBilling
      ? seedanceReferenceVideoTiers(row.provider, model)
      : []
    const savedBillingMeter =
      price?.billing_meter === 'image' && supportsImageBilling
        ? 'image'
        : price?.billing_meter === 'video' && supportsSeedanceVideoBilling
          ? 'video'
          : price?.billing_meter === 'token' && !supportsSeedanceVideoBilling
            ? 'token'
            : null
    const billingMeter = savedBillingMeter ?? defaultBillingMeterForModel(row.provider, model)
    const videoBillingMode =
      supportsSeedanceVideoBilling && price?.video_billing_mode
        ? price.video_billing_mode
        : supportsSeedanceVideoBilling
          ? ('official_token' as const)
          : null
    const initialVideoPriceTiers =
      supportsSeedanceVideoBilling && price?.video_price_tiers?.length
        ? videoPriceTiersToForm(price.video_price_tiers, referenceVideoTiers)
        : supportsSeedanceVideoBilling && referenceVideoTiers.length > 0
          ? referenceVideoTiersToForm(referenceVideoTiers)
          : []
    const inputPrice = price?.input_price_micros ?? template?.input_price_micros ?? 0
    const cacheWritePrice = template
      ? template.cache_write_price_micros
      : (price?.cache_write_price_micros ?? inputPrice)
    priceForms[key] = {
      provider: row.provider,
      model,
      billingMeter,
      videoBillingMode,
      videoPriceTiers: initialVideoPriceTiers,
      inputPerMillion: microAmountToMajor(inputPrice),
      outputPerMillion: microAmountToMajor(
        price?.output_price_micros ?? template?.output_price_micros ?? 0
      ),
      cacheReadPerMillion: microAmountToMajor(
        price?.cache_read_price_micros ??
          template?.cache_read_price_micros ??
          derivedCacheReadPrice(inputPrice)
      ),
      cacheWritePerMillion:
        cacheWritePrice === undefined || cacheWritePrice === null
          ? 0
          : microAmountToMajor(cacheWritePrice),
      unitPrice: microAmountToMajor(price?.unit_price_micros ?? template?.unit_price_micros ?? 0),
      enabled: hasEnabledBillablePrice(price, billingMeter) || Boolean(template),
      hasPrice: hasEnabledBillablePrice(price, billingMeter),
      hasPriceRecord: Boolean(price),
      billingMeterLocked: isBillingMeterLocked(row.provider, model),
      canUseImageBilling: supportsImageBilling,
      canUseSeedanceVideoBilling: supportsSeedanceVideoBilling
    }
  }
  priceDialogOpen.value = true
}

function hasReferencePrice(form: (typeof priceForms)[string]) {
  if (form.canUseSeedanceVideoBilling) {
    return seedanceReferenceVideoTiers(form.provider, form.model).length > 0
  }
  return Boolean(findApplicablePricingTemplate(form))
}

function referencePriceFallbackLabel(form: (typeof priceForms)[string]) {
  return form.hasPrice ? t('referencePriceNotSynced') : t('priceMissing')
}

function referencePriceSummary(form: (typeof priceForms)[string]) {
  if (form.canUseSeedanceVideoBilling) {
    const tiers = seedanceReferenceVideoTiers(form.provider, form.model)
    if (tiers.length > 0) {
      return tiers
        .map((tier) => {
          const resolutionLabel =
            referenceVideoTierResolutionLabel(tier) ||
            referenceVideoTierResolutions(tier).join(', ')
          return `${resolutionLabel} ${referenceVideoTierSummary(tier)}`
        })
        .join('\n')
    }
  }
  const template = findApplicablePricingTemplate(form)
  if (!template) return ''
  if (template.billing_meter === 'image') {
    const unit = template.unit_price_micros
      ? formatPricePerMillion(template.unit_price_micros, locale.value)
      : t('priceMissing')
    return `${t('billingMeterImageGeneration')} ${unit} / ${t('perImage')}`
  }
  const input = formatPricePerMillion(template.input_price_micros, locale.value)
  const output = formatPricePerMillion(template.output_price_micros, locale.value)
  const cacheRead = formatPricePerMillion(
    template.cache_read_price_micros ?? derivedCacheReadPrice(template.input_price_micros),
    locale.value
  )
  const cacheWrite =
    template.cache_write_price_micros === undefined || template.cache_write_price_micros === null
      ? `${currencySymbol.value}0`
      : formatPricePerMillion(template.cache_write_price_micros, locale.value)
  return `Token ${input} / ${output}\nCache ${cacheRead} / ${cacheWrite}`
}

function videoTierReferencePriceSummary(form: ChannelPriceForm, tier: ChannelVideoPriceTierForm) {
  if (!form.canUseSeedanceVideoBilling) return ''
  const resolutions = new Set(
    splitCommaList(tier.resolutionsText).map((resolution) => resolution.toLowerCase())
  )
  const referenceTier = seedanceReferenceVideoTiers(form.provider, form.model).find((item) => {
    return referenceVideoTierMatchesFormResolutions(item, resolutions)
  })
  if (!referenceTier) return ''

  return referenceVideoTierSummary(referenceTier)
}

function fillReferencePrice(form: (typeof priceForms)[string]) {
  if (form.canUseSeedanceVideoBilling) {
    const tiers = seedanceReferenceVideoTiers(form.provider, form.model)
    if (tiers.length === 0) return
    form.videoBillingMode = 'official_token'
    form.billingMeter = 'video'
    form.videoPriceTiers = referenceVideoTiersToForm(tiers)
    const payload = videoPriceTiersPayload(form)
    const representative = representativeVideoPriceMicros(payload, form.videoBillingMode)
    form.inputPerMillion = microAmountToMajor(representative)
    form.outputPerMillion = microAmountToMajor(representative)
    form.cacheReadPerMillion = 0
    form.cacheWritePerMillion = 0
    form.unitPrice = 0
    return
  }
  const template = findApplicablePricingTemplate(form)
  if (!template) return
  form.billingMeter = template.billing_meter
  form.inputPerMillion = microAmountToMajor(template.input_price_micros)
  form.outputPerMillion = microAmountToMajor(template.output_price_micros)
  form.cacheReadPerMillion = microAmountToMajor(
    template.cache_read_price_micros ?? derivedCacheReadPrice(template.input_price_micros)
  )
  form.cacheWritePerMillion =
    template.cache_write_price_micros === undefined || template.cache_write_price_micros === null
      ? 0
      : microAmountToMajor(template.cache_write_price_micros)
  form.unitPrice = microAmountToMajor(template.unit_price_micros ?? 0)
}

function cacheWritePricePayload(form: (typeof priceForms)[string]) {
  return form.cacheWritePerMillion === null ? null : majorToMicroAmount(form.cacheWritePerMillion)
}

function requireBillingMeter(form: (typeof priceForms)[string]) {
  if (!form.billingMeter) {
    throw new Error(t('billingMeterRequired'))
  }
  return form.billingMeter
}

function readReferenceSyncError(err: unknown) {
  if (
    err instanceof ApiError &&
    err.status === 502 &&
    err.message.includes('pricing reference source')
  ) {
    return t('referencePricesSourceUnavailable')
  }

  return readError(err)
}

function createFormMissingReferencePrices() {
  const models = splitCommaList(createForm.models)
  return models.some((model) => !findPricingTemplate(templates.value, createForm.provider, model))
}

async function syncCreateReferencePricesIfNeeded() {
  if (!createFormMissingReferencePrices()) return true

  try {
    await syncPricingTemplates()
    const [
      fetchedTemplates,
      fetchedProviderModels,
      fetchedModelReferenceCatalog,
      fetchedBundledVideoTierCatalog
    ] = await Promise.all([
      getPricingTemplates(),
      getProviderModels(),
      getModelReferenceCatalog(),
      getBundledVideoTierCatalog()
    ])
    templates.value = fetchedTemplates
    providerModels.value = fetchedProviderModels
    modelReferenceCatalog.value = fetchedModelReferenceCatalog
    bundledVideoTierCatalog.value = fetchedBundledVideoTierCatalog
    return true
  } catch (err) {
    ElMessage.error(readReferenceSyncError(err))
    return false
  }
}

async function saveChannelPrices() {
  await withLoading(savingPrices, async () => {
    try {
      for (const form of Object.values(priceForms)) {
        if (!shouldSavePriceForm(form)) continue
        const videoTiers =
          form.canUseSeedanceVideoBilling && form.videoBillingMode
            ? videoPriceTiersPayload(form)
            : []
        const billingMeter =
          form.canUseSeedanceVideoBilling && form.videoBillingMode === 'per_second'
            ? 'video'
            : requireBillingMeter(form)
        const representativeWithoutVideo = representativeVideoPriceMicros(
          videoTiers,
          form.videoBillingMode
        )
        const representativeWithVideo = representativeVideoPriceMicros(
          videoTiers,
          form.videoBillingMode,
          true
        )
        await upsertProviderPrice({
          provider: form.provider,
          model: form.model,
          input_price_micros:
            form.canUseSeedanceVideoBilling && form.videoBillingMode
              ? representativeWithoutVideo
              : majorToMicroAmount(form.inputPerMillion),
          output_price_micros:
            form.canUseSeedanceVideoBilling && form.videoBillingMode
              ? representativeWithVideo
              : majorToMicroAmount(form.outputPerMillion),
          cache_read_price_micros:
            form.canUseSeedanceVideoBilling && form.videoBillingMode
              ? 0
              : majorToMicroAmount(form.cacheReadPerMillion),
          cache_write_price_micros:
            form.canUseSeedanceVideoBilling && form.videoBillingMode
              ? 0
              : cacheWritePricePayload(form),
          billing_meter: billingMeter,
          unit_price_micros:
            billingMeter === 'image'
              ? majorToMicroAmount(form.unitPrice)
              : billingMeter === 'video'
                ? representativeWithoutVideo
                : null,
          video_billing_mode:
            form.canUseSeedanceVideoBilling && form.videoBillingMode ? form.videoBillingMode : null,
          video_price_tiers: videoTiers,
          enabled: shouldEnablePriceForm(form)
        })
      }
      ElMessage.success(t('priceSaved'))
      await loadPricingData()
      await loadChannels()
      priceDialogOpen.value = false
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function toggleChannelModelRuntime(channelId: number, model: string, enabled: boolean) {
  if (isRuntimeToggling(channelId, model)) return

  await togglingRuntimeKeys.withItem(runtimeKey(channelId, model), async () => {
    try {
      await updateChannelModel(channelId, model, { enabled })
      await loadChannels()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function toggleChannelRuntime(row: Channel, enabled: boolean) {
  if (row.enabled === enabled || isChannelToggling(row.id)) return

  await togglingChannelIds.withItem(row.id, async () => {
    try {
      await updateChannel(row.id, {
        name: row.name,
        endpoints: row.endpoints.map((endpoint) => ({
          protocol: endpoint.protocol,
          base_url: endpoint.base_url,
          models: endpoint.models,
          enabled: endpoint.enabled
        })),
        enabled,
        priority: row.priority,
        weight: row.weight,
        key_selection_mode: row.key_selection_mode,
        use_credentials: row.use_credentials
      })
      await loadChannels()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function applyReferencePrices() {
  const targetForms = Object.values(priceForms).filter(hasReferencePrice)
  if (targetForms.length === 0) {
    ElMessage.warning(t('noReferencePrices'))
    return
  }

  for (const form of targetForms) {
    fillReferencePrice(form)
  }
  ElMessage.success(t('referencePricesApplied'))
}

async function submitChannel() {
  normalizeCredentialModeForServiceMode(createForm)
  const channel = await submitChannelBase(syncCreateReferencePricesIfNeeded)
  if (!channel) return
  await loadPricingData()
  if (channelPriceStatus(channel).missing > 0) {
    openPriceDialog(channel)
  }
}

async function submitEditChannel() {
  normalizeCredentialModeForServiceMode(editForm)
  const channel = await submitEditChannelBase()
  if (!channel) return
  await loadPricingData()
}

function normalizeCredentialModeForServiceMode(form: typeof createForm) {
  if (isInternalServiceMode.value) {
    form.use_credentials = false
  }
}

function openCreateChannelDialog() {
  openCreateDialog()
  normalizeCredentialModeForServiceMode(createForm)
}

function openEditChannelDialog(row: Channel) {
  openEditDialog(row)
  normalizeCredentialModeForServiceMode(editForm)
}

async function loadInitialData() {
  try {
    await Promise.all([
      loadChannels(),
      loadPricingData(),
      getAdminServicePolicy().then((policy) => {
        servicePolicy.value = policy
      })
    ])
  } finally {
    channelsLoaded.value = true
  }
}

onMounted(loadInitialData)
</script>

<template>
  <section class="grid channel-management-view">
    <el-form class="channel-toolbar" @submit.prevent="searchChannels">
      <div class="channel-toolbar-filters">
        <label class="admin-filter-field channel-search-field">
          <span>{{ t('name') }}</span>
          <el-input
            v-model="channelSearch"
            class="channel-search-input"
            clearable
            :placeholder="t('channelSearchPlaceholder')"
            :prefix-icon="Search"
            @clear="clearChannelSearch"
          />
        </label>
        <label class="admin-filter-field">
          <span>{{ t('channelStatus') }}</span>
          <el-select v-model="channelStatusFilter" class="channel-status-filter">
            <el-option :label="t('allStatus')" value="all" />
            <el-option :label="t('channelRunningNormal')" value="normal" />
            <el-option :label="t('channelNeedsAttention')" value="attention" />
            <el-option :label="t('channelStopped')" value="disabled" />
          </el-select>
        </label>
        <el-button
          class="admin-action-button channel-search-action"
          type="primary"
          :icon="Search"
          :loading="loading"
          native-type="submit"
        >
          {{ t('search') }}
        </el-button>
      </div>
      <div class="channel-toolbar-actions">
        <el-button
          class="admin-action-button add-channel-action"
          type="primary"
          :icon="Plus"
          @click="openCreateChannelDialog"
        >
          {{ t('addChannel') }}
        </el-button>
      </div>
    </el-form>

    <div v-if="!channelsLoaded" class="service-table-panel">
      <div v-loading="true" class="admin-table service-table channel-table-loading">
        <div class="channel-table-loading-head">
          <span></span>
          <span></span>
          <span></span>
          <span></span>
        </div>
        <div class="channel-table-loading-row"></div>
        <div class="channel-table-loading-row"></div>
      </div>
    </div>

    <div v-else class="service-table-panel" :class="{ 'has-pagination': channelHasPagination }">
      <el-table
        ref="channelTableRef"
        v-loading="loading"
        class="admin-table service-table channel-table"
        :data="paginatedChannels"
        :row-class-name="channelRowClassName"
        row-key="id"
        stripe
        @expand-change="handleChannelExpandChange"
      >
        <el-table-column type="expand" width="44">
          <template #default="{ row }">
            <ChannelExpandPanel
              :channel="row"
              :rows="channelPriceRows(row)"
              @configure-price="openPriceDialog"
              @toggle-model-runtime="toggleChannelModelRuntime"
            />
          </template>
        </el-table-column>
        <el-table-column prop="id" label="ID" width="48" align="right" header-align="right" />
        <el-table-column prop="name" :label="t('name')" min-width="150" header-align="center">
          <template #default="{ row }">
            <button
              type="button"
              class="channel-expand-toggle channel-name-cell"
              :aria-label="`${row.name} ${t('modelPriceDetails')}`"
              @click="toggleChannelRowExpansion(row)"
            >
              <ProviderIcon :provider="row.provider" />
              <span class="channel-name-stack">
                <span class="channel-name-text">{{ row.name }}</span>
                <span class="channel-provider-text">{{ row.provider }}</span>
              </span>
            </button>
          </template>
        </el-table-column>
        <el-table-column :label="t('modelPrices')" min-width="200">
          <template #default="{ row }">
            <div class="channel-price-summary">
              <div v-if="channelPriceStatus(row).missing > 0" class="channel-price-summary-head">
                <el-tag
                  class="price-status-tag"
                  :class="`is-${channelPriceStatus(row).type}`"
                  round
                >
                  <el-icon>
                    <WarningFilled />
                  </el-icon>
                  {{ channelPriceStatus(row).label }}
                </el-tag>
              </div>
              <div class="channel-price-list">
                <button
                  v-for="item in channelPricePreviewRows(row)"
                  :key="item.model"
                  class="channel-price-item"
                  :class="{ 'is-missing': item.missing, 'is-disabled': item.disabled }"
                  type="button"
                  :aria-label="`${item.model} ${t('modelPriceDetails')}`"
                  @click="toggleChannelRowExpansion(row)"
                >
                  <span class="channel-price-model">{{ item.model }}</span>
                  <span class="channel-price-value">{{ item.price }}</span>
                </button>
                <button
                  v-if="channelPriceOverflowCount(row) > 0"
                  class="channel-price-more"
                  type="button"
                  :aria-label="`${row.name} ${t('modelPriceDetails')}`"
                  @click="toggleChannelRowExpansion(row)"
                >
                  +{{ channelPriceOverflowCount(row) }}
                </button>
              </div>
            </div>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelKeyCountShort')"
          min-width="92"
          class-name="channel-key-count-column"
          label-class-name="channel-key-count-header"
        >
          <template #default="{ row }">
            <span class="channel-key-count">{{ channelCredentialSummary(row).label }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('probeTrend')"
          min-width="120"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <ChannelProbeTrendCell :channel="row" :latency-scale="probeTrendLatencyScale" />
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelStatus')"
          min-width="96"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span class="channel-runtime-status-tag" :class="`is-${channelRuntimeStatus(row)}`">
              <template v-if="channelRuntimeStatus(row) === 'normal'">
                {{ t('channelRunningNormal') }}
              </template>
              <template v-else-if="channelRuntimeStatus(row) === 'attention'">
                {{ t('channelNeedsAttention') }}
              </template>
              <template v-else>
                {{ t('channelStopped') }}
              </template>
            </span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelRuntimeSwitch')"
          min-width="116"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <button
              type="button"
              class="channel-runtime-switch"
              :class="{ 'is-enabled': row.enabled, 'is-disabled': !row.enabled }"
              :disabled="isChannelToggling(row.id)"
              :aria-pressed="row.enabled"
              :aria-label="row.enabled ? t('enabled') : t('disabled')"
              @click="toggleChannelRuntime(row, !row.enabled)"
            >
              <span class="channel-runtime-switch-icon">
                <el-icon>
                  <CircleCheckFilled v-if="row.enabled" />
                  <VideoPause v-else />
                </el-icon>
              </span>
              <span class="channel-runtime-switch-text">
                {{ row.enabled ? t('enabled') : t('disabled') }}
              </span>
            </button>
          </template>
        </el-table-column>
        <el-table-column :label="t('actions')" min-width="176" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('fullDiagnoseChannel')"
                :disabled="diagnosticInProgress"
                :icon="Lightning"
                :loading="isChannelDiagnosing(row.id)"
                @click="runChannelDiagnostic(row)"
              >
                {{ t('actionDiagnose') }}
              </el-button>
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('edit')"
                :icon="Edit"
                @click="openEditChannelDialog(row)"
              >
                {{ t('actionEdit') }}
              </el-button>
              <el-button
                class="admin-action-button compact-row-action"
                type="danger"
                :aria-label="t('delete')"
                :disabled="deletingId === row.id"
                :icon="Delete"
                @click="confirmDeleteChannel(row)"
              >
                {{ t('actionDelete') }}
              </el-button>
            </div>
          </template>
        </el-table-column>
        <template #empty>
          <div class="channel-empty-state">
            <el-empty
              :description="hasChannelSearchCriteria ? t('noMatchingChannels') : t('noChannels')"
            />
          </div>
        </template>
      </el-table>
    </div>

    <div
      v-if="channelsLoaded && !loading && channelHasPagination"
      class="admin-pagination-bar admin-table-pagination is-compact"
    >
      <div class="admin-pagination-controls">
        <div class="admin-page-size-control">
          <span class="admin-page-label">{{ t('pageSize') }}</span>
          <el-select
            v-model="channelPageSize"
            class="admin-page-size"
            @change="handleChannelPageSizeChange"
          >
            <el-option v-for="s in channelPageSizes" :key="s" :value="s" :label="String(s)" />
          </el-select>
        </div>
        <div class="admin-page-buttons">
          <el-button
            :aria-label="t('previousPage')"
            :disabled="channelCurrentPage <= 1"
            @click="channelCurrentPage--"
          >
            <el-icon><ArrowLeft /></el-icon>
          </el-button>
          <span class="admin-page-current">{{ channelCurrentPage }} / {{ channelTotalPages }}</span>
          <el-button
            :aria-label="t('nextPage')"
            :disabled="channelCurrentPage >= channelTotalPages"
            @click="channelCurrentPage++"
          >
            <el-icon><ArrowRight /></el-icon>
          </el-button>
        </div>
      </div>
    </div>

    <ChannelFormDialog
      v-model:open="createDialogOpen"
      v-model:form="createForm"
      v-model:base-url="createBaseUrl"
      v-model:secret="secretInput"
      mode="create"
      :provider-options="providerOptions"
      :provider-value="createForm.provider"
      :fetching-models="fetchingModels"
      :submitting="creating"
      :models-input-placeholder="modelsInputPlaceholder()"
      :models-input-readonly="modelsInputReadonly()"
      :secret-placeholder="t('optionalUpstreamKey')"
      :hide-credential-files-toggle="isInternalServiceMode"
      @fetch-models="fetchCreateModels"
      @select-provider="selectCreateProvider"
      @submit="submitChannel"
    />

    <ModelPickerDialog
      v-model:open="modelPickerDialogOpen"
      v-model:models="fetchedModels"
      v-model:selected-models="selectedFetchedModels"
      :all-selected="allFetchedModelsSelected"
      @toggle-all="toggleAllFetchedModels"
    />

    <ChannelFormDialog
      v-model:open="editDialogOpen"
      v-model:form="editForm"
      v-model:base-url="editBaseUrl"
      v-model:secret="editSecretInput"
      mode="edit"
      :provider-options="providerOptions"
      :provider-value="editingChannel?.provider ?? ''"
      :fetching-models="fetchingModels"
      :submitting="updating"
      :models-input-placeholder="modelsInputPlaceholder()"
      :models-input-readonly="modelsInputReadonly()"
      :secret-placeholder="t('optionalEditUpstreamKey')"
      :hide-credential-files-toggle="isInternalServiceMode"
      :existing-keys="editingChannelKeys"
      :deleting-key-id="deletingKeyId"
      :copying-key-id="copyingKeyId"
      @fetch-models="fetchEditModels"
      @copy-key="copyChannelKeySecret"
      @delete-key="confirmDeleteChannelKey"
      @submit="submitEditChannel"
    />

    <ChannelPriceDialog
      v-model:open="priceDialogOpen"
      :forms="priceForms"
      :saving="savingPrices"
      :has-reference-price="hasReferencePrice"
      :reference-price-summary="referencePriceSummary"
      :video-tier-reference-price-summary="videoTierReferencePriceSummary"
      :reference-price-fallback-label="referencePriceFallbackLabel"
      @apply-reference-prices="applyReferencePrices"
      @save="saveChannelPrices"
    />

    <ChannelDiagnosticDialog :diagnostic="diagnostic" @retry="runChannelDiagnostic" />
  </section>
</template>

<style scoped>
.channel-search-input {
  flex: 0 0 min(260px, 100%);
  width: min(260px, 100%);
}

.channel-search-field {
  flex: 0 1 310px;
}

.channel-search-action {
  flex: 0 0 auto;
}

.channel-status-filter {
  width: 150px;
}

.channel-name-cell {
  align-items: center;
  display: inline-flex;
  gap: 2px;
  max-width: 100%;
  min-width: 0;
  vertical-align: middle;
}

.channel-name-cell :deep(.provider-icon) {
  border-radius: 8px;
  flex: 0 0 auto;
  height: 40px;
  width: auto;
}

.channel-name-cell :deep(.provider-icon.has-image img) {
  height: 30px;
  width: 30px;
}

.channel-name-cell :deep(.provider-icon .provider-icon-symbol) {
  height: 22px;
  width: 22px;
}

.channel-name-stack {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.channel-name-text {
  color: #1d2129;
  font-size: 14px;
  font-weight: 680;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-provider-text {
  color: #86909c;
  font-size: 12px;
  font-weight: 560;
  line-height: 1.15;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-table-loading {
  min-height: 214px;
  overflow: hidden;
}

.channel-table-loading-head {
  align-items: center;
  background: #f6f9fc;
  border-bottom: 1px solid #dfe8f2;
  display: grid;
  gap: 36px;
  grid-template-columns: 160px minmax(200px, 1fr) 120px 140px;
  height: 48px;
  padding: 0 72px 0 82px;
}

.channel-table-loading-head span,
.channel-table-loading-row::before,
.channel-table-loading-row::after {
  background: #e8eef6;
  border-radius: 999px;
  content: '';
  display: block;
  height: 12px;
}

.channel-table-loading-head span:nth-child(1) {
  width: 52px;
}

.channel-table-loading-head span:nth-child(2) {
  width: 220px;
}

.channel-table-loading-head span:nth-child(3) {
  width: 82px;
}

.channel-table-loading-head span:nth-child(4) {
  width: 48px;
}

.channel-table-loading-row {
  align-items: center;
  border-bottom: 1px solid #edf3f8;
  display: grid;
  gap: 36px;
  grid-template-columns: 160px minmax(200px, 1fr);
  height: 82px;
  padding: 0 72px 0 82px;
}

.channel-table-loading-row::before {
  width: 136px;
}

.channel-table-loading-row::after {
  width: min(420px, 100%);
}

.channel-table :deep(.el-table__body td) {
  height: 82px;
  padding: 12px 0;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled td) {
  background: #f8fafc;
  color: #94a3b8;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-name-text),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-provider-text),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-price-model),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-price-value),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-key-count),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-runtime-switch-text) {
  color: #94a3b8;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-runtime-switch) {
  border-color: #e5e7eb;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-runtime-switch-icon) {
  background: #cbd5e1;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-runtime-switch) {
  border-color: #e5e7eb;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-runtime-switch-icon) {
  background: #cbd5e1;
}

.channel-table :deep(.el-table__expanded-cell) {
  background: #f6f9fc;
  height: auto !important;
  max-width: 100%;
  overflow: hidden;
  padding: 0 !important;
}

.channel-table :deep(.el-table__expanded-cell .cell) {
  max-width: 100%;
  overflow: hidden;
}

.channel-table :deep(.el-table__expand-icon) {
  border-radius: 6px;
  color: #86909c;
  height: 28px;
  transition:
    background-color 160ms ease,
    color 160ms ease,
    transform 180ms ease;
  width: 28px;
}

.channel-table :deep(.el-table__expand-icon .el-icon) {
  transition: transform 180ms ease;
}

.channel-table :deep(.el-table__expand-icon:hover) {
  background: var(--brand-blue-soft);
  color: var(--brand-blue);
}

.channel-table :deep(.el-table__expand-icon--expanded) {
  background: var(--brand-blue-soft);
  color: var(--brand-blue);
}

.channel-table :deep(.channel-key-count-header .cell) {
  white-space: nowrap;
}

.channel-table :deep(.el-table__body .cell) {
  align-items: center;
  display: flex;
}

.channel-expand-toggle {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  color: inherit;
  cursor: pointer;
  display: flex;
  font: inherit;
  gap: 8px;
  min-width: 0;
  padding: 0;
  text-align: left;
  width: 100%;
}

.channel-expand-toggle:focus-visible {
  outline: 2px solid var(--brand-blue);
  outline-offset: 3px;
}

.channel-expand-toggle:hover .channel-name-text,
.channel-expand-toggle:hover .channel-price-model {
  color: var(--brand-blue);
}

.channel-table :deep(.el-table__expand-column .cell) {
  justify-content: center;
}

.channel-table :deep(.el-table__body .el-table__cell:nth-child(3) .cell) {
  display: block;
}

.channel-price-summary {
  display: grid;
  gap: 8px;
  min-width: 0;
  width: 100%;
}

.channel-price-summary-head {
  align-items: center;
  display: flex;
  gap: 8px;
  min-width: 0;
}

.price-status-tag.is-success {
  background: #f0fdf4;
  border-color: #bbf7d0;
  color: #15803d;
}

.price-status-tag.is-success .el-icon {
  background: #22c55e;
}

.price-status-tag.is-warning {
  background: #fffbeb;
  border-color: #fde68a;
  color: #a16207;
}

.price-status-tag.is-warning .el-icon {
  background: #eab308;
}

.price-status-tag.is-danger {
  background: #fff7ed;
  border-color: #fed7aa;
  color: #c2410c;
}

.price-status-tag.is-danger .el-icon {
  background: #f97316;
}

.price-status-tag.is-info {
  background: #f1f5f9;
  border-color: #dbe4ef;
  color: #64748b;
}

.channel-price-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  justify-items: start;
  max-width: 100%;
  min-width: 0;
  width: fit-content;
}

.channel-price-item {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  cursor: pointer;
  display: inline-flex;
  font: inherit;
  gap: 0;
  inline-size: fit-content;
  min-width: 0;
  overflow: hidden;
  padding: 0;
  width: fit-content;
}

.channel-price-item:focus-visible,
.channel-price-more:focus-visible {
  outline: 2px solid var(--brand-blue);
  outline-offset: 2px;
}

.channel-price-item:hover .channel-price-model {
  color: var(--brand-blue);
}

.channel-price-model {
  background: var(--admin-primary-soft);
  border: 1px solid var(--admin-primary-border);
  border-radius: 999px 0 0 999px;
  color: var(--admin-primary);
  font-size: 12px;
  font-weight: 680;
  letter-spacing: 0;
  max-width: 148px;
  overflow: hidden;
  padding: 2px 8px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-price-value {
  background: #ffffff;
  border: 1px solid #dfe8f2;
  border-left: 0;
  border-radius: 0 999px 999px 0;
  color: #1d2939;
  font-size: 12px;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 680;
  padding: 2px 9px;
  text-align: left;
  white-space: nowrap;
}

.channel-price-item.is-missing .channel-price-model {
  background: #fff7ed;
  border-color: #fed7aa;
  color: #c2410c;
}

.channel-price-item.is-missing .channel-price-value {
  background: #f97316;
  border-color: #f97316;
  color: #ffffff;
}

.channel-price-item.is-disabled {
  opacity: 0.56;
}

.channel-price-more {
  align-items: center;
  appearance: none;
  background: #f1f5f9;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: #64748b;
  cursor: pointer;
  display: inline-flex;
  font-size: 12px;
  font-weight: 720;
  min-height: 24px;
  padding: 0 9px;
}

.channel-price-more:hover {
  border-color: #b9dff3;
  color: var(--brand-blue);
}

.channel-key-count {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: #4e5969;
  display: inline-flex;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-weight: 700;
  justify-content: center;
  min-height: 28px;
  min-width: 0;
  padding: 0 10px;
  white-space: nowrap;
}

.channel-runtime-status-tag {
  border-radius: 999px;
  display: inline-block;
  font-size: 12px;
  font-weight: 700;
  line-height: 1;
  padding: 5px 12px;
  white-space: nowrap;
}

.channel-runtime-status-tag.is-normal {
  background: #f0fdf4;
  color: #15803d;
}

.channel-runtime-status-tag.is-attention {
  background: #fff7ed;
  color: #c2410c;
}

.channel-runtime-status-tag.is-disabled {
  background: #f1f5f9;
  color: #64748b;
}

.channel-runtime-switch.is-enabled,
.channel-runtime-switch.is-enabled .channel-runtime-switch-text {
  background: var(--admin-success-bg);
  border-color: var(--admin-success-border);
  color: var(--admin-success);
}

.channel-runtime-switch.is-disabled,
.channel-runtime-switch.is-disabled .channel-runtime-switch-text {
  background: var(--admin-danger-bg);
  border-color: var(--admin-danger-border);
  color: var(--admin-danger);
}

.channel-runtime-switch.is-enabled .channel-runtime-switch-icon {
  background: #22c55e;
}

.channel-runtime-switch.is-disabled .channel-runtime-switch-icon {
  background: #94a3b8;
}

.channel-empty-state {
  padding: 30px 0 34px;
}
</style>
