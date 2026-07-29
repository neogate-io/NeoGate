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
  getChannelPrices,
  syncPricingTemplates,
  upsertChannelPrice
} from '../../api/prices'
import { updateChannelModel, updateChannel, type ChannelDiagnosticScope } from '../../api/channels'
import { getAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import ChannelFormDialog from '../../components/admin/channels/ChannelFormDialog.vue'
import ChannelDiagnosticDialog from '../../components/admin/channels/ChannelDiagnosticDialog.vue'
import ChannelExpandPanel, {
  type ChannelExpandPriceGroup
} from '../../components/admin/channels/ChannelExpandPanel.vue'
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
import type { MessageKey } from '../../i18n'
import type {
  BillingMeter,
  Channel,
  ChannelKey,
  ChannelModel,
  ModelReferenceCatalogRecord,
  PricingTemplate,
  ProviderModel,
  ChannelPrice,
  VideoBillingMode,
  VideoPriceTier
} from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { splitCommaList } from '../../utils/channel'
import {
  channelPriceKey,
  derivedCacheReadPrice,
  findPricingTemplate,
  isChannelPriceConfigured,
  priceKey,
  pricingReferenceModelAliases,
  resolvedVideoTokensPerSecondEstimate
} from '../../utils/pricing'
import {
  ANY_VIDEO_TIER_RESOLUTION,
  defaultVideoBillingModeForReferenceTiers,
  isAnyReferenceVideoTier,
  lockedVideoBillingModeForReferenceTiers,
  referenceVideoPriceShape,
  referenceVideoTierHasAudio,
  referenceVideoTierInputWithVideo,
  referenceVideoTierInputWithoutVideo,
  referenceVideoTierMatchesFormResolutions,
  referenceVideoTierResolutions,
  referenceVideoTierUsesInputOutputLabels,
  referenceVideoTierUsesSinglePrice,
  referenceVideoTierUsesSingleTokenPrice,
  savedVideoTierMatchesReferenceTier,
  type ReferenceVideoTier
} from '../../utils/videoPricing'

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

const prices = ref<ChannelPrice[]>([])
const templates = ref<PricingTemplate[]>([])
const providerModels = ref<ProviderModel[]>([])
const modelReferenceCatalog = ref<ModelReferenceCatalogRecord[]>([])
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
const diagnosticScopeDialogOpen = ref(false)
const diagnosticScopeChannel = ref<Channel | null>(null)
const selectedDiagnosticScope = ref<ChannelDiagnosticScope>('all')
const channelCurrentPage = ref(1)
const channelPageSize = ref(20)
const channelPageSizes = [20, 50, 100]
const priceForms = reactive<Record<string, ChannelPriceForm>>({})
const anyVideoTierResolution = ANY_VIDEO_TIER_RESOLUTION

const priceByModel = computed(
  () =>
    new Map(prices.value.map((price) => [channelPriceKey(price.channel_id, price.model), price]))
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

const baseDiagnosticScopeOptions: Array<{ value: ChannelDiagnosticScope; labelKey: MessageKey }> = [
  { value: 'all', labelKey: 'diagnosticScopeAll' },
  { value: 'models', labelKey: 'diagnosticScopeModels' },
  { value: 'text', labelKey: 'diagnosticScopeText' },
  { value: 'image', labelKey: 'diagnosticScopeImage' },
  { value: 'video', labelKey: 'diagnosticScopeVideo' }
]

const diagnosticScopeOptions = computed(() => {
  const row = diagnosticScopeChannel.value
  if (!row) return baseDiagnosticScopeOptions
  const capabilities = channelDiagnosticCapabilities(row)
  return baseDiagnosticScopeOptions.filter((option) => {
    if (option.value === 'text') return capabilities.text
    if (option.value === 'image') return capabilities.image
    if (option.value === 'video') return capabilities.video
    return true
  })
})

function openDiagnosticScopeDialog(row: Channel) {
  if (diagnosticInProgress.value) return
  diagnosticScopeChannel.value = row
  selectedDiagnosticScope.value = diagnosticScopeOptions.value[0]?.value ?? 'models'
  diagnosticScopeDialogOpen.value = true
}

function startChannelDiagnosticWithScope() {
  const row = diagnosticScopeChannel.value
  if (!row) return
  const scope = selectedDiagnosticScope.value
  diagnosticScopeDialogOpen.value = false
  diagnosticScopeChannel.value = null
  void runChannelDiagnostic(row, scope)
}

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
        billing_enabled: Boolean(
          priceByModel.value.get(channelPriceKey(row.id, model))?.enabled &&
          isChannelPriceConfigured(priceByModel.value.get(channelPriceKey(row.id, model)))
        ),
        price_configured: Boolean(priceByModel.value.get(channelPriceKey(row.id, model))),
        created_at: '',
        updated_at: ''
      }) as ChannelModel
  )
}

function channelDiagnosticCapabilities(row: Channel) {
  const models = channelModelRecords(row)
  return models.reduce(
    (capabilities, item) => {
      const meter =
        item.billing_meter ??
        priceByModel.value.get(channelPriceKey(row.id, item.model))?.billing_meter
      const displayMeter = displayBillingMeterForModel(row.provider, item.model, meter)
      if (displayMeter === 'image') {
        capabilities.image = true
      } else if (displayMeter === 'video') {
        capabilities.video = true
      } else {
        capabilities.text = true
      }
      return capabilities
    },
    { text: false, image: false, video: false }
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
    const price = priceByModel.value.get(channelPriceKey(row.id, model))
    const hasConfiguredPrice = channelModel.price_configured || isChannelPriceConfigured(price)
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
        category: 'text' as const,
        billingMeterLabel: '-',
        inputPrice: '-',
        outputPrice: '-',
        cacheReadPrice: '-',
        cacheWritePrice: '-',
        cachePrice: '-',
        imagePriceGroups: [],
        videoBillingMode: '-',
        videoTiers: [],
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
    const modelCategory = modelCategoryForModel(row.provider, model)
    const defaultBillingMeter = defaultBillingMeterForModel(row.provider, model)
    const displayBillingMeter = billingMeter ?? defaultBillingMeter
    const billingMeterLabel = billingMeterDisplayLabel(row.provider, model, displayBillingMeter)
    const formattedPriceParts = {
      input: inputPrice,
      output: outputPrice,
      cacheRead: cacheReadPrice,
      cacheWrite: cacheWritePrice,
      unit: unitPrice
    }
    const imageGroups = imagePriceGroups(modelCategory, displayBillingMeter, Boolean(price), {
      ...formattedPriceParts
    })
    const videoTiers =
      modelCategory === 'video' && displayBillingMeter === 'token'
        ? [
            {
              specs: '-',
              price: price
                ? `${formattedPriceParts.input} / ${formattedPriceParts.output}`
                : t('priceMissing')
            }
          ]
        : displayBillingMeter === 'video'
          ? videoTierRows(row.provider, model, price)
          : []
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
      category: modelCategory,
      billingMeterLabel,
      modelStatus,
      modelStatusLabel: modelStatusLabel(modelStatus),
      inputPrice,
      outputPrice,
      cacheReadPrice,
      cacheWritePrice,
      imagePriceGroups: imageGroups,
      cachePrice:
        displayBillingMeter === 'image' ||
        displayBillingMeter === 'video' ||
        displayBillingMeter === 'audio'
          ? '-'
          : price && displayBillingMeter === 'token'
            ? `${cacheReadPrice} / ${cacheWritePrice}`
            : t('priceMissing'),
      videoBillingMode:
        modelCategory === 'video' ? videoBillingModeDisplayLabel(price, displayBillingMeter) : '-',
      videoTiers,
      price:
        displayBillingMeter === 'image'
          ? `${unitPrice} / ${t('perImage')}`
          : displayBillingMeter === 'audio'
            ? `${unitPrice} / ${t('perSecond')}`
            : displayBillingMeter === 'video'
              ? videoTierSpecsLabel(price?.video_price_tiers)
              : price && displayBillingMeter === 'token'
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

function inlinePriceGroup(label: string, price: string): ChannelExpandPriceGroup {
  return { label, price, inline: true }
}

function compactPriceGroup(items: ChannelExpandPriceGroup[]) {
  return items.length > 0
    ? [
        inlinePriceGroup(
          items.map((item) => item.label).join(' / '),
          items.map((item) => item.price).join(' / ')
        )
      ]
    : []
}

function imagePriceGroups(
  modelCategory: 'text' | 'image' | 'video' | 'audio',
  displayBillingMeter: BillingMeter,
  configured: boolean,
  prices: {
    input: string
    output: string
    cacheRead: string
    cacheWrite: string
    unit: string
  }
) {
  if (modelCategory === 'audio') {
    return [inlinePriceGroup(`/ ${t('perSecond')}`, configured ? prices.unit : t('priceMissing'))]
  }
  if (modelCategory !== 'image') return []
  if (displayBillingMeter === 'token') {
    return [
      inlinePriceGroup(
        t('inputOutputPairShort'),
        configured ? `${prices.input} / ${prices.output}` : t('priceMissing')
      ),
      inlinePriceGroup(
        t('cacheReadWritePairShort'),
        configured ? `${prices.cacheRead} / ${prices.cacheWrite}` : t('priceMissing')
      )
    ]
  }
  return [
    inlinePriceGroup(
      t('perImage'),
      displayBillingMeter === 'image' ? prices.unit : t('priceMissing')
    )
  ]
}

function matchingReferenceVideoTierForSavedTier(
  provider: string,
  model: string,
  tier: VideoPriceTier
) {
  const formResolutions = new Set(
    tier.resolutions.flatMap(splitCommaList).map((item) => item.toLowerCase())
  )
  return priceDialogReferenceVideoTiersForModel(provider, model).find((referenceTier) =>
    referenceVideoTierMatchesFormResolutions(referenceTier, formResolutions)
  )
}

function videoTierRows(provider: string, model: string, price?: ChannelPrice) {
  const mode = price?.video_billing_mode ?? 'official_token'
  const suffix = mode === 'per_second' ? t('perSecond') : t('pricePerMillionTokens')
  return (price?.video_price_tiers ?? []).map((tier) => {
    const referenceTier = matchingReferenceVideoTierForSavedTier(provider, model, tier)
    const singleTokenPriceTier =
      referenceTier !== undefined && referenceVideoTierUsesSingleTokenPrice(referenceTier)
    const singlePriceTier =
      referenceTier !== undefined && referenceVideoPriceShape(referenceTier) === 'single'
    const specs = singleTokenPriceTier ? '-' : videoTierSpecsLabel([tier])
    const primary =
      mode === 'per_second' ? tier.input_without_video_unit_micros : tier.input_without_video_micros
    const secondary =
      mode === 'per_second' ? tier.input_with_video_unit_micros : tier.input_with_video_micros
    const singlePrice = primary ?? secondary
    if (singlePriceTier && singlePrice !== undefined && singlePrice !== null) {
      return {
        specs,
        priceGroups: [inlinePriceGroup(`/ ${suffix}`, formatPricePerMillion(singlePrice, 'en-US'))],
        price: ''
      }
    }
    const values = [primary, secondary]
      .filter((value): value is number => value !== undefined && value !== null)
      .filter((value, index, list) => list.indexOf(value) === index)
    const officialTokenPrices = [
      primary !== undefined && primary !== null
        ? inlinePriceGroup(t('videoInputWithoutVideo'), formatPricePerMillion(primary, 'en-US'))
        : null,
      secondary !== undefined && secondary !== null
        ? inlinePriceGroup(t('videoInputWithVideo'), formatPricePerMillion(secondary, 'en-US'))
        : null
    ].filter((group): group is ChannelExpandPriceGroup => Boolean(group))
    const officialTokenPriceGroups =
      mode === 'official_token' ? compactPriceGroup(officialTokenPrices) : []
    const perSecondPrices = [
      primary !== undefined && primary !== null
        ? inlinePriceGroup(t('videoInputWithoutVideo'), formatPricePerMillion(primary, 'en-US'))
        : null,
      secondary !== undefined && secondary !== null
        ? inlinePriceGroup(t('videoInputWithVideo'), formatPricePerMillion(secondary, 'en-US'))
        : null
    ].filter((group): group is ChannelExpandPriceGroup => Boolean(group))
    const perSecondPriceGroups =
      mode === 'per_second' && perSecondPrices.length > 0 ? compactPriceGroup(perSecondPrices) : []
    return {
      specs,
      priceGroups:
        officialTokenPriceGroups.length > 0 ? officialTokenPriceGroups : perSecondPriceGroups,
      price:
        officialTokenPriceGroups.length > 0 || perSecondPriceGroups.length > 0
          ? ''
          : values.length > 0
            ? `${values.map((value) => formatPricePerMillion(value, 'en-US')).join(' / ')} / ${suffix}`
            : t('priceMissing')
    }
  })
}

function videoTierSpecsLabel(tiers: VideoPriceTier[] = []) {
  const specs = tiers
    .flatMap((tier) => tier.resolutions)
    .map((resolution) => resolution.trim())
    .filter(Boolean)
    .map((resolution) =>
      resolution === anyVideoTierResolution ? t('videoTierAllSpecifications') : resolution
    )
  const uniqueSpecs = Array.from(new Set(specs))
  return uniqueSpecs.length > 0 ? uniqueSpecs.join(' / ') : t('billingBasisMultiTierVideo')
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
      const [fetchedPrices, fetchedTemplates, fetchedProviderModels, fetchedModelReferenceCatalog] =
        await Promise.all([
          getChannelPrices(),
          getPricingTemplates(),
          getProviderModels(),
          getModelReferenceCatalog()
        ])
      prices.value = fetchedPrices
      templates.value = fetchedTemplates
      providerModels.value = fetchedProviderModels
      modelReferenceCatalog.value = fetchedModelReferenceCatalog
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
  if (!template) return output

  const referenceRecord = findReferenceProviderModel(template.provider, template.model)
  return referenceRecord
    ? capabilityValues(referenceRecord.capabilities, ['modalities', 'output'])
    : output
}

function canUseImageBilling(provider: string, model: string) {
  const output = modelOutputModalities(provider, model)
  return output.length === 1 && output[0] === 'image'
}

function canUseVideoBilling(provider: string, model: string) {
  const output = modelOutputModalities(provider, model)
  return output.includes('video') || referenceVideoTiersForModel(provider, model).length > 0
}

function hasAudioTranscriptionCapability(capabilities: Record<string, unknown> | undefined) {
  if (!capabilities) return false
  if (capabilities.audio_transcription === true) return true
  const input = capabilityValues(capabilities, ['modalities', 'input'])
  const output = capabilityValues(capabilities, ['modalities', 'output'])
  return input.includes('audio') && output.includes('text')
}

function canUseAudioBilling(provider: string, model: string) {
  const catalogRecord = findReferenceCatalogRecord(provider, model)
  const providerRecord = findReferenceProviderModel(provider, model)
  return (
    hasAudioTranscriptionCapability(catalogRecord?.capabilities) ||
    hasAudioTranscriptionCapability(providerRecord?.capabilities)
  )
}

function modelCategoryForModel(
  provider: string,
  model: string
): 'text' | 'image' | 'video' | 'audio' {
  if (canUseAudioBilling(provider, model)) return 'audio'
  const output = modelOutputModalities(provider, model)
  if (output.includes('video') || referenceVideoTiersForModel(provider, model).length > 0) {
    return 'video'
  }
  if (output.length === 1 && output[0] === 'image') return 'image'
  return 'text'
}

function defaultBillingMeterForModel(provider: string, model: string) {
  if (canUseAudioBilling(provider, model)) return 'audio'
  if (canUseVideoBilling(provider, model)) {
    const template = findPricingTemplate(templates.value, provider, model)
    if (referenceVideoTiersForModel(provider, model).length === 0 && template?.billing_meter) {
      return template.billing_meter
    }
    return 'video'
  }
  return canUseImageBilling(provider, model) ? 'image' : 'token'
}

function displayBillingMeterForModel(
  provider: string,
  model: string,
  billingMeter?: BillingMeter | null
) {
  const defaultMeter = defaultBillingMeterForModel(provider, model)
  if (defaultMeter !== 'token') return defaultMeter
  return billingMeter ?? defaultMeter
}

function billingMeterDisplayLabel(provider: string, model: string, billingMeter: BillingMeter) {
  if (canUseImageBilling(provider, model) && billingMeter === 'token')
    return t('pricePerMillionTokens')
  if (canUseImageBilling(provider, model) && billingMeter === 'image')
    return t('billingMeterPerCall')
  if (billingMeter === 'image') return t('billingMeterImageGeneration')
  if (billingMeter === 'video') return t('billingMeterVideo')
  if (billingMeter === 'audio') return t('videoBillingPerSecond')
  return t('billingMeterToken')
}

function videoBillingModeDisplayLabel(price: ChannelPrice | undefined, billingMeter: BillingMeter) {
  if (price?.video_billing_mode === 'per_second') return t('videoBillingPerSecond')
  if (price?.video_billing_mode === 'official_token' || billingMeter === 'video') {
    return t('pricePerMillionTokens')
  }
  return billingMeter === 'token' ? t('pricePerMillionTokens') : t('billingMeterVideo')
}

function isBillingMeterLocked(provider: string, model: string) {
  return (
    canUseAudioBilling(provider, model) ||
    canUseVideoBilling(provider, model) ||
    !canUseImageBilling(provider, model)
  )
}

function templateAppliesToForm(template: PricingTemplate, form: ChannelPriceForm) {
  if (form.canUseVideoBilling && template.billing_meter === 'token') return true
  return (
    template.billing_meter ===
    (form.billingMeter ?? defaultBillingMeterForModel(form.provider, form.model))
  )
}

function findApplicablePricingTemplate(form: ChannelPriceForm) {
  const template = findPricingTemplate(templates.value, form.provider, form.model)
  return template && templateAppliesToForm(template, form) ? template : undefined
}

function lockedBillingMeterForReferencePrice(provider: string, model: string): BillingMeter | null {
  if (!canUseImageBilling(provider, model)) return null
  const template = findPricingTemplate(templates.value, provider, model)
  return template?.billing_meter === 'image' ? 'image' : null
}

function hasManualPriceInput(form: ChannelPriceForm) {
  if (form.canUseVideoBilling && form.videoBillingMode) {
    return form.videoPriceTiers.some((tier) => {
      if (form.videoBillingMode === 'official_token') {
        return tier.inputWithVideo > 0 || tier.inputWithoutVideo > 0
      }
      return tier.inputWithVideoUnit > 0 || tier.inputWithoutVideoUnit > 0
    })
  }
  if (form.billingMeter === 'image' || form.billingMeter === 'audio') return form.unitPrice > 0
  return (
    form.inputPerMillion > 0 ||
    form.outputPerMillion > 0 ||
    form.cacheReadPerMillion > 0 ||
    (form.cacheWritePerMillion ?? 0) > 0
  )
}

function hasEnabledBillablePrice(price?: ChannelPrice, billingMeter?: BillingMeter | null) {
  if (!price?.enabled) return false
  if (billingMeter && price.billing_meter !== billingMeter) return false
  if (price.video_billing_mode) {
    return (price.video_price_tiers ?? []).length > 0
  }
  if (price.billing_meter === 'image' || price.billing_meter === 'audio') {
    return (price.unit_price_micros ?? 0) > 0
  }
  if (price.billing_meter === 'video') return (price.video_price_tiers ?? []).length > 0
  return price.input_price_micros > 0 || price.output_price_micros > 0
}

function shouldSavePriceForm(form: ChannelPriceForm) {
  return form.hasPriceRecord || hasReferencePrice(form) || hasManualPriceInput(form)
}

function shouldEnablePriceForm(form: ChannelPriceForm) {
  return form.enabled || hasManualPriceInput(form)
}

function canonicalReferenceProvider(provider: string) {
  const normalizedProvider = provider.trim().toLowerCase()
  if (
    normalizedProvider === 'qwen' ||
    normalizedProvider === 'dashscope' ||
    normalizedProvider === 'alibaba'
  ) {
    return 'qwen'
  }
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

function referenceVideoTierSummary(tier: ReferenceVideoTier) {
  const pricePerMillionTokensUnit = t('pricePerMillionTokens').replace(' ', '\u00a0')
  if (referenceVideoTierUsesSinglePrice(tier)) {
    return `${t('videoTierPrice')} ${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}/${t('perSecond')}`
  }

  if (referenceVideoTierUsesSingleTokenPrice(tier)) {
    return `${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}/${pricePerMillionTokensUnit}`
  }

  if (referenceVideoTierUsesInputOutputLabels(tier)) {
    return `${t('inputPriceShort')} ${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}/${pricePerMillionTokensUnit}\n${t('outputPriceShort')} ${currencySymbol.value}${referenceVideoTierInputWithVideo(tier)}/${pricePerMillionTokensUnit}`
  }

  if (referenceVideoTierHasAudio(tier)) {
    return `${t('videoTierWithAudio')} ${currencySymbol.value}${referenceVideoTierInputWithVideo(tier)}/${pricePerMillionTokensUnit}\n${t('videoTierWithoutAudio')} ${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}/${pricePerMillionTokensUnit}`
  }

  const leftLabel = referenceVideoTierHasAudio(tier)
    ? t('videoTierWithoutAudio')
    : t('videoInputWithoutVideo')
  const rightLabel = referenceVideoTierHasAudio(tier)
    ? t('videoTierWithAudio')
    : t('videoInputWithVideo')
  return `${leftLabel} ${currencySymbol.value}${referenceVideoTierInputWithoutVideo(tier)}/${pricePerMillionTokensUnit}\n${rightLabel} ${currencySymbol.value}${referenceVideoTierInputWithVideo(tier)}/${pricePerMillionTokensUnit}`
}

function referenceVideoTiersForModel(provider: string, model: string) {
  const catalogRecord = findReferenceCatalogRecord(provider, model)
  const catalogValue = catalogRecord?.capabilities?.video_tiers
  if (Array.isArray(catalogValue) && catalogValue.length > 0) {
    return filterReferenceVideoTiersForCurrency(
      normalizedReferenceVideoTiers(model, catalogValue as ReferenceVideoTier[])
    )
  }

  const record = findReferenceProviderModel(provider, model)
  const value = record?.capabilities?.video_tiers
  if (Array.isArray(value) && value.length > 0) {
    return filterReferenceVideoTiersForCurrency(
      normalizedReferenceVideoTiers(model, value as ReferenceVideoTier[])
    )
  }

  const template = findPricingTemplate(templates.value, provider, model)
  if (template && template.provider !== provider) {
    return referenceVideoTiersForModel(template.provider, model)
  }

  return []
}

function videoTokenReferenceTierFromTemplate(template?: PricingTemplate): ReferenceVideoTier[] {
  if (!template || template.billing_meter !== 'token') return []
  return [
    {
      pricePairKind: 'input_output',
      tiers: {
        input_without_video: microAmountToMajor(template.input_price_micros),
        input_with_video: microAmountToMajor(template.output_price_micros)
      }
    }
  ]
}

function priceDialogReferenceVideoTiersForModel(provider: string, model: string) {
  const tiers = referenceVideoTiersForModel(provider, model)
  if (tiers.length > 0) return tiers
  return videoTokenReferenceTierFromTemplate(findPricingTemplate(templates.value, provider, model))
}

function isMainlandReferenceVideoTier(tier: ReferenceVideoTier) {
  const label = tier.label?.trim() ?? ''
  return (
    label.includes('中国内地') ||
    label.includes('中国大陆') ||
    /mainland\s*china|china\s*mainland|chinese\s*mainland/i.test(label)
  )
}

function referenceVideoTierDedupeKey(tier: ReferenceVideoTier) {
  return referenceVideoTierResolutions(tier)
    .map((resolution) => resolution.trim().toLowerCase())
    .sort()
    .join('|')
}

function dedupeReferenceVideoTiersByResolution(tiers: ReferenceVideoTier[]) {
  const seen = new Set<string>()
  return tiers.filter((tier) => {
    const key = referenceVideoTierDedupeKey(tier)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function filterReferenceVideoTiersForCurrency(tiers: ReferenceVideoTier[]) {
  if (billingCurrency.value !== 'CNY') return dedupeReferenceVideoTiersByResolution(tiers)
  const mainlandTiers = tiers.filter(isMainlandReferenceVideoTier)
  return dedupeReferenceVideoTiersByResolution(mainlandTiers.length > 0 ? mainlandTiers : tiers)
}

function normalizedReferenceVideoTiers(model: string, tiers: ReferenceVideoTier[]) {
  const aliases = pricingReferenceModelAliases(model)
  const isSeedanceFastOrMini = [...aliases].some((alias) =>
    /(?:^|-)seedance-2\.0-(?:fast|mini)$/.test(alias)
  )
  if (!isSeedanceFastOrMini) return tiers
  return tiers.map((tier) =>
    tier.resolution?.trim() ? tier : { ...tier, resolution: '480p,720p' }
  )
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

function videoPriceTierPairLabels(referenceTier?: ReferenceVideoTier) {
  if (!referenceTier) return {}
  if (referenceVideoTierUsesInputOutputLabels(referenceTier)) {
    return {
      pricePairLeftLabel: t('inputPriceShort'),
      pricePairRightLabel: t('outputPriceShort')
    }
  }
  if (!referenceVideoTierHasAudio(referenceTier)) return {}
  return {
    pricePairLeftLabel: t('videoTierWithoutAudio'),
    pricePairRightLabel: t('videoTierWithAudio')
  }
}

function videoPriceTierFormFromSavedTier(
  tier: VideoPriceTier | undefined,
  resolution: string,
  referenceTier?: ReferenceVideoTier
) {
  const usesSinglePrice = referenceTier
    ? referenceVideoPriceShape(referenceTier) === 'single'
    : false
  const usesSingleUnitPrice = referenceTier
    ? referenceVideoTierUsesSinglePrice(referenceTier)
    : false
  const referencePrice = {
    inputWithVideo: referenceTier ? referenceVideoTierInputWithVideo(referenceTier) : 0,
    inputWithoutVideo: referenceTier ? referenceVideoTierInputWithoutVideo(referenceTier) : 0
  }
  return {
    resolutionsText: resolution,
    resolutionLabel: referenceTier
      ? referenceVideoTierResolutionLabel(referenceTier)
      : videoTierFormResolutionLabel(resolution),
    ...videoPriceTierPairLabels(referenceTier),
    singlePrice: usesSinglePrice,
    inputWithVideo:
      microAmountToMajor(tier?.input_with_video_micros ?? 0) ||
      (usesSingleUnitPrice ? 0 : referencePrice.inputWithVideo),
    inputWithoutVideo:
      microAmountToMajor(tier?.input_without_video_micros ?? 0) ||
      (usesSingleUnitPrice ? 0 : referencePrice.inputWithoutVideo),
    estimatedTokensPerSecond: resolvedVideoTokensPerSecondEstimate(
      referenceTier && isAnyReferenceVideoTier(referenceTier)
        ? null
        : tier?.estimated_tokens_per_second,
      resolution
    ),
    inputWithVideoUnit:
      microAmountToMajor(tier?.input_with_video_unit_micros ?? 0) ||
      (usesSingleUnitPrice ? referencePrice.inputWithVideo : 0),
    inputWithoutVideoUnit:
      microAmountToMajor(tier?.input_without_video_unit_micros ?? 0) ||
      (usesSingleUnitPrice ? referencePrice.inputWithoutVideo : 0)
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

  if (referenceTiers.length > 0) {
    return referenceTiers.flatMap((referenceTier) =>
      referenceVideoTierResolutions(referenceTier).map((resolution) => {
        const savedTier = tiers.find((tier) =>
          savedVideoTierMatchesReferenceTier(tier, referenceTier)
        )
        return videoPriceTierFormFromSavedTier(savedTier, resolution, referenceTier)
      })
    )
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
    referenceVideoTierResolutions(tier).map((resolution) =>
      videoPriceTierFormFromSavedTier(undefined, resolution, tier)
    )
  )
}

function videoPriceTiersPayload(form: ChannelPriceForm): VideoPriceTier[] {
  return form.videoPriceTiers
    .map((tier) => {
      const inputWithVideo = tier.singlePrice ? tier.inputWithoutVideo : tier.inputWithVideo
      const inputWithVideoUnit = tier.singlePrice
        ? tier.inputWithoutVideoUnit
        : tier.inputWithVideoUnit
      return {
        resolutions: splitCommaList(tier.resolutionsText),
        input_with_video_micros:
          form.videoBillingMode === 'official_token' ? majorToMicroAmount(inputWithVideo) : null,
        input_without_video_micros:
          form.videoBillingMode === 'official_token'
            ? majorToMicroAmount(tier.inputWithoutVideo)
            : null,
        estimated_tokens_per_second:
          form.videoBillingMode === 'official_token'
            ? Math.max(1, Math.round(tier.estimatedTokensPerSecond || 0))
            : null,
        input_with_video_unit_micros:
          form.videoBillingMode === 'per_second' ? majorToMicroAmount(inputWithVideoUnit) : null,
        input_without_video_unit_micros:
          form.videoBillingMode === 'per_second'
            ? majorToMicroAmount(tier.inputWithoutVideoUnit)
            : null
      }
    })
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
    const key = channelPriceKey(row.id, model)
    const price = priceByModel.value.get(key)
    const template = findPricingTemplate(templates.value, row.provider, model)
    const supportsImageBilling = canUseImageBilling(row.provider, model)
    const supportsVideoBilling = canUseVideoBilling(row.provider, model)
    const referenceVideoTiers = supportsVideoBilling
      ? priceDialogReferenceVideoTiersForModel(row.provider, model)
      : []
    const lockedBillingMeter = lockedBillingMeterForReferencePrice(row.provider, model)
    const lockedVideoBillingMode = lockedVideoBillingModeForReferenceTiers(referenceVideoTiers)
    const defaultBillingMeter = defaultBillingMeterForModel(row.provider, model)
    const savedBillingMeter =
      price?.billing_meter === 'image' && supportsImageBilling
        ? 'image'
        : price?.billing_meter === 'video' && supportsVideoBilling
          ? 'video'
          : price?.billing_meter === 'audio' && defaultBillingMeter === 'audio'
            ? 'audio'
            : price?.billing_meter === 'token' && defaultBillingMeter === 'token'
              ? 'token'
              : null
    const billingMeter =
      supportsVideoBilling && referenceVideoTiers.length > 0
        ? 'video'
        : (lockedBillingMeter ?? savedBillingMeter ?? defaultBillingMeter)
    const videoBillingMode =
      lockedVideoBillingMode ??
      (supportsVideoBilling && price?.video_billing_mode
        ? price.video_billing_mode
        : supportsVideoBilling && referenceVideoTiers.length > 0
          ? defaultVideoBillingModeForReferenceTiers(referenceVideoTiers)
          : null)
    const initialVideoPriceTiers =
      supportsVideoBilling && price?.video_price_tiers?.length
        ? videoPriceTiersToForm(price.video_price_tiers, referenceVideoTiers)
        : supportsVideoBilling && referenceVideoTiers.length > 0
          ? referenceVideoTiersToForm(referenceVideoTiers)
          : []
    const inputPrice = price?.input_price_micros ?? template?.input_price_micros ?? 0
    const cacheWritePrice = template
      ? (template.cache_write_price_micros ?? price?.cache_write_price_micros)
      : (price?.cache_write_price_micros ?? inputPrice)
    priceForms[key] = {
      channelId: row.id,
      provider: row.provider,
      model,
      modelCategory: modelCategoryForModel(row.provider, model),
      billingMeter,
      videoBillingMode,
      videoPriceTiers: initialVideoPriceTiers,
      inputPerMillion: microAmountToMajor(inputPrice),
      outputPerMillion: microAmountToMajor(
        price?.output_price_micros ?? template?.output_price_micros ?? 0
      ),
      cacheReadPerMillion: microAmountToMajor(
        price?.cache_read_price_micros ?? template?.cache_read_price_micros ?? 0
      ),
      cacheWritePerMillion:
        cacheWritePrice === undefined || cacheWritePrice === null
          ? 0
          : microAmountToMajor(cacheWritePrice),
      unitPrice: microAmountToMajor(price?.unit_price_micros ?? template?.unit_price_micros ?? 0),
      enabled: hasEnabledBillablePrice(price, billingMeter) || Boolean(template),
      hasPrice: hasEnabledBillablePrice(price, billingMeter),
      hasPriceRecord: Boolean(price),
      billingMeterLocked: Boolean(lockedBillingMeter) || isBillingMeterLocked(row.provider, model),
      videoBillingModeLocked: Boolean(lockedVideoBillingMode),
      canUseImageBilling: supportsImageBilling,
      canUseVideoBilling: supportsVideoBilling
    }
  }
  priceDialogOpen.value = true
}

function hasReferencePrice(form: (typeof priceForms)[string]) {
  if (form.canUseVideoBilling) {
    return (
      priceDialogReferenceVideoTiersForModel(form.provider, form.model).length > 0 ||
      Boolean(findApplicablePricingTemplate(form))
    )
  }
  return Boolean(findApplicablePricingTemplate(form))
}

function referencePriceFallbackLabel(form: (typeof priceForms)[string]) {
  return form.hasPrice ? t('referencePriceNotSynced') : t('priceMissing')
}

function referencePriceSummary(form: (typeof priceForms)[string]) {
  if (form.canUseVideoBilling) {
    const tiers = priceDialogReferenceVideoTiersForModel(form.provider, form.model)
    if (tiers.length > 0) {
      return tiers
        .map((tier) => {
          const resolutionLabel =
            referenceVideoTierResolutionLabel(tier) ||
            referenceVideoTierResolutions(tier).join(', ')
          if (referenceVideoTierUsesSingleTokenPrice(tier)) {
            return referenceVideoTierSummary(tier)
          }
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
  if (template.billing_meter === 'audio') {
    const unit = template.unit_price_micros
      ? formatPricePerMillion(template.unit_price_micros, locale.value)
      : t('priceMissing')
    return `${t('billingMeterAudio')} ${unit} / ${t('perSecond')}`
  }
  const input = formatPricePerMillion(template.input_price_micros, locale.value)
  const output = formatPricePerMillion(template.output_price_micros, locale.value)
  const cachePrices = [template.cache_read_price_micros, template.cache_write_price_micros]
  const hasCachePrice = cachePrices.some((value) => value !== undefined && value !== null)
  if (!hasCachePrice) return `Token ${input} / ${output}`

  const cacheRead =
    template.cache_read_price_micros === undefined || template.cache_read_price_micros === null
      ? `${currencySymbol.value}0`
      : formatPricePerMillion(template.cache_read_price_micros, locale.value)
  const cacheWrite =
    template.cache_write_price_micros === undefined || template.cache_write_price_micros === null
      ? `${currencySymbol.value}0`
      : formatPricePerMillion(template.cache_write_price_micros, locale.value)
  return `Token ${input} / ${output}\nCache ${cacheRead} / ${cacheWrite}`
}

function videoTierReferencePriceSummary(form: ChannelPriceForm, tier: ChannelVideoPriceTierForm) {
  if (!form.canUseVideoBilling) return ''
  const resolutions = new Set(
    splitCommaList(tier.resolutionsText).map((resolution) => resolution.toLowerCase())
  )
  const referenceTier = priceDialogReferenceVideoTiersForModel(form.provider, form.model).find(
    (item) => {
      return referenceVideoTierMatchesFormResolutions(item, resolutions)
    }
  )
  if (!referenceTier) return ''

  return referenceVideoTierSummary(referenceTier)
}

function fillReferencePrice(form: (typeof priceForms)[string]) {
  if (form.canUseVideoBilling) {
    const tiers = priceDialogReferenceVideoTiersForModel(form.provider, form.model)
    if (tiers.length > 0) {
      form.videoBillingMode = defaultVideoBillingModeForReferenceTiers(tiers)
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
  }
  const template = findApplicablePricingTemplate(form)
  if (!template) return
  form.billingMeter = template.billing_meter
  form.inputPerMillion = microAmountToMajor(template.input_price_micros)
  form.outputPerMillion = microAmountToMajor(template.output_price_micros)
  form.cacheReadPerMillion = microAmountToMajor(template.cache_read_price_micros ?? 0)
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

function requireUnitPrice(form: (typeof priceForms)[string]) {
  if (form.unitPrice <= 0) {
    throw new Error(
      form.billingMeter === 'audio' ? t('audioUnitPriceRequired') : t('imageUnitPriceRequired')
    )
  }
}

function readReferenceSyncError(err: unknown) {
  if (err instanceof ApiError && err.code === 'pricing_reference_source_unavailable') {
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
    const [fetchedTemplates, fetchedProviderModels, fetchedModelReferenceCatalog] =
      await Promise.all([getPricingTemplates(), getProviderModels(), getModelReferenceCatalog()])
    templates.value = fetchedTemplates
    providerModels.value = fetchedProviderModels
    modelReferenceCatalog.value = fetchedModelReferenceCatalog
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
          form.canUseVideoBilling && form.videoBillingMode ? videoPriceTiersPayload(form) : []
        const billingMeter =
          form.canUseVideoBilling && form.videoBillingMode ? 'video' : requireBillingMeter(form)
        if (billingMeter === 'image' || billingMeter === 'audio') {
          requireUnitPrice(form)
        }
        const representativeWithoutVideo = representativeVideoPriceMicros(
          videoTiers,
          form.videoBillingMode
        )
        const representativeWithVideo = representativeVideoPriceMicros(
          videoTiers,
          form.videoBillingMode,
          true
        )
        await upsertChannelPrice({
          channel_id: form.channelId,
          model: form.model,
          input_price_micros:
            form.canUseVideoBilling && form.videoBillingMode
              ? representativeWithoutVideo
              : majorToMicroAmount(form.inputPerMillion),
          output_price_micros:
            form.canUseVideoBilling && form.videoBillingMode
              ? representativeWithVideo
              : majorToMicroAmount(form.outputPerMillion),
          cache_read_price_micros:
            form.canUseVideoBilling && form.videoBillingMode
              ? 0
              : majorToMicroAmount(form.cacheReadPerMillion),
          cache_write_price_micros:
            form.canUseVideoBilling && form.videoBillingMode ? 0 : cacheWritePricePayload(form),
          billing_meter: billingMeter,
          unit_price_micros:
            billingMeter === 'image' || billingMeter === 'audio'
              ? majorToMicroAmount(form.unitPrice)
              : billingMeter === 'video'
                ? representativeWithoutVideo
                : null,
          video_billing_mode:
            form.canUseVideoBilling && form.videoBillingMode ? form.videoBillingMode : null,
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
              @edit-price="openPriceDialog"
              @toggle-model-runtime="toggleChannelModelRuntime"
            />
          </template>
        </el-table-column>
        <el-table-column prop="id" label="ID" width="48" align="right" header-align="right" />
        <el-table-column
          prop="name"
          :label="t('channelName')"
          width="220"
          header-align="center"
          class-name="channel-name-column"
          label-class-name="channel-name-header"
        >
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
        <el-table-column :label="t('modelInfo')" min-width="260">
          <template #default="{ row }">
            <button
              type="button"
              class="channel-expand-toggle channel-price-summary"
              :aria-label="`${row.name} ${t('modelPriceDetails')}`"
              @click="toggleChannelRowExpansion(row)"
            >
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
                <span
                  v-for="item in channelPricePreviewRows(row)"
                  :key="item.model"
                  class="channel-price-item"
                  :class="{ 'is-missing': item.missing, 'is-disabled': item.disabled }"
                >
                  <span class="channel-price-model">{{ item.model }}</span>
                </span>
                <span v-if="channelPriceOverflowCount(row) > 0" class="channel-price-more">
                  +{{ channelPriceOverflowCount(row) }}
                </span>
              </div>
            </button>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelKeyCountShort')"
          width="112"
          align="center"
          header-align="center"
          class-name="channel-key-count-column"
          label-class-name="channel-key-count-header"
        >
          <template #default="{ row }">
            <span class="channel-key-count">{{ channelCredentialSummary(row).label }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('probeTrend')"
          width="176"
          align="center"
          header-align="center"
          class-name="channel-probe-trend-column"
          label-class-name="channel-probe-trend-header"
        >
          <template #default="{ row }">
            <ChannelProbeTrendCell :channel="row" :latency-scale="probeTrendLatencyScale" />
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelStatus')"
          width="112"
          align="center"
          header-align="center"
          class-name="channel-runtime-status-column"
          label-class-name="channel-runtime-status-header"
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
          width="124"
          align="center"
          header-align="center"
          class-name="channel-runtime-switch-column"
          label-class-name="channel-runtime-switch-header"
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
        <el-table-column
          :label="t('actions')"
          width="196"
          align="center"
          header-align="center"
          class-name="channel-actions-column"
          label-class-name="channel-actions-header"
        >
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('fullDiagnoseChannel')"
                :disabled="diagnosticInProgress"
                :icon="Lightning"
                :loading="isChannelDiagnosing(row.id)"
                @click="openDiagnosticScopeDialog(row)"
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

    <el-dialog
      v-model="diagnosticScopeDialogOpen"
      class="channel-dialog diagnostic-scope-dialog"
      :title="t('diagnosticScopeTitle')"
      width="420px"
      :close-on-click-modal="false"
    >
      <div class="diagnostic-scope-content">
        <div v-if="diagnosticScopeChannel" class="diagnostic-scope-channel">
          <strong>{{ diagnosticScopeChannel.name }}</strong>
          <span>{{ diagnosticScopeChannel.provider }}</span>
        </div>
        <el-radio-group v-model="selectedDiagnosticScope" class="diagnostic-scope-options">
          <el-radio
            v-for="option in diagnosticScopeOptions"
            :key="option.value"
            :value="option.value"
          >
            {{ t(option.labelKey) }}
          </el-radio>
        </el-radio-group>
      </div>
      <template #footer>
        <el-button @click="diagnosticScopeDialogOpen = false">{{ t('cancel') }}</el-button>
        <el-button
          type="primary"
          :disabled="diagnosticInProgress || diagnosticScopeOptions.length === 0"
          @click="startChannelDiagnosticWithScope"
        >
          {{ t('actionDiagnose') }}
        </el-button>
      </template>
    </el-dialog>

    <ChannelDiagnosticDialog :diagnostic="diagnostic" @retry="runChannelDiagnostic" />
  </section>
</template>

<style scoped>
.diagnostic-scope-content {
  display: grid;
  gap: 16px;
}

.diagnostic-scope-channel {
  background: #f7f8fa;
  border: 1px solid #e5e6eb;
  border-radius: 8px;
  display: grid;
  gap: 4px;
  padding: 10px 12px;
}

.diagnostic-scope-channel strong {
  color: #1d2129;
  font-size: 14px;
  font-weight: 720;
}

.diagnostic-scope-channel span {
  color: #86909c;
  font-size: 12px;
}

.diagnostic-scope-options {
  align-items: flex-start;
  display: grid;
  gap: 10px;
}

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
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-provider-text {
  color: #86909c;
  font-size: 12px;
  font-weight: 400;
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

.channel-table :deep(.el-table__header .cell) {
  color: #64748b;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0;
}

.channel-table :deep(.el-table__body .cell) {
  align-items: center;
  color: #344054;
  display: flex;
  font-size: 13px;
  font-weight: 400;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled td) {
  background: #f8fafc;
  color: #94a3b8;
}

.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-name-text),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-provider-text),
.channel-table :deep(.el-table__body tr.channel-row-is-disabled .channel-price-model),
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
  background: #ffffff;
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

.channel-table
  :deep(
    .channel-name-column .cell,
    .channel-key-count-column .cell,
    .channel-probe-trend-column .cell,
    .channel-runtime-status-column .cell,
    .channel-runtime-switch-column .cell,
    .channel-actions-column .cell
  ) {
  justify-content: center;
  min-width: 0;
  overflow: hidden;
  padding-left: 8px;
  padding-right: 8px;
}

.channel-table
  :deep(
    .channel-name-header .cell,
    .channel-key-count-header .cell,
    .channel-probe-trend-header .cell,
    .channel-runtime-status-header .cell,
    .channel-runtime-switch-header .cell,
    .channel-actions-header .cell
  ) {
  justify-content: center;
  min-width: 0;
  overflow: hidden;
  text-overflow: clip;
  white-space: nowrap;
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

.channel-table :deep(.channel-actions-column .table-row-actions) {
  justify-content: center;
  width: 172px;
}

.channel-price-summary {
  display: grid;
  gap: 8px;
  justify-items: start;
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
  display: inline-flex;
  font: inherit;
  gap: 0;
  inline-size: fit-content;
  min-width: 0;
  overflow: hidden;
  padding: 0;
  width: fit-content;
}

.channel-price-item:hover .channel-price-model {
  color: var(--brand-blue);
}

.channel-price-model {
  background: var(--admin-primary-soft);
  border: 1px solid var(--admin-primary-border);
  border-radius: 999px;
  color: var(--admin-primary);
  font-size: 12px;
  font-weight: 400;
  letter-spacing: 0;
  max-width: 240px;
  overflow: hidden;
  padding: 2px 8px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-price-item.is-missing .channel-price-model {
  background: #fff7ed;
  border-color: #fed7aa;
  color: #c2410c;
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
  display: inline-flex;
  font-size: 12px;
  font-weight: 400;
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
  font-weight: 400;
  justify-content: center;
  min-height: 28px;
  width: 74px;
  padding: 0 10px;
  white-space: nowrap;
}

.channel-runtime-status-tag {
  border-radius: 999px;
  display: inline-block;
  font-size: 12px;
  font-weight: 400;
  line-height: 1;
  padding: 5px 12px;
  white-space: nowrap;
  width: 76px;
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

.channel-runtime-switch {
  width: 92px;
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
