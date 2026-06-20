<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Coin,
  Delete,
  Edit,
  MoreFilled,
  Plus,
  Search,
  VideoPause,
  WarningFilled
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  getProviderModels,
  getPricingTemplates,
  getProviderPrices,
  syncPricingTemplates,
  upsertProviderPrice
} from '../../api/prices'
import { updateChannelModel, updateChannel } from '../../api/channels'
import ChannelFormDialog from '../../components/admin/channels/ChannelFormDialog.vue'
import ChannelDiagnosticDialog from '../../components/admin/channels/ChannelDiagnosticDialog.vue'
import ChannelExpandPanel from '../../components/admin/channels/ChannelExpandPanel.vue'
import ChannelProbeTrendCell from '../../components/admin/channels/ChannelProbeTrendCell.vue'
import ChannelPriceDialog, {
  type ChannelPriceForm
} from '../../components/admin/channels/ChannelPriceDialog.vue'
import ModelPickerDialog from '../../components/admin/channels/ModelPickerDialog.vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useChannelDiagnostics } from '../../composables/useChannelDiagnostics'
import { useChannels } from '../../composables/useChannels'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type {
  BillingMeter,
  Channel,
  ChannelKey,
  ChannelModel,
  PricingTemplate,
  ProviderModel,
  ProviderPrice
} from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { formatUsdPerMillion, microUsdToUsd, usdToMicroUsd } from '../../utils/format'
import { splitCommaList } from '../../utils/channel'
import {
  derivedCacheReadPrice,
  findPricingTemplate,
  isProviderPriceConfigured,
  priceKey
} from '../../utils/pricing'

const { t } = useLocale()

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
  isCreateBaseUrlReadonly,
  isEditBaseUrlReadonly,
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
const pricingLoading = ref(true)
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

const priceByModel = computed(
  () => new Map(prices.value.map((price) => [priceKey(price.provider, price.model), price]))
)
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

    const inputMicros = channelModel.input_price_usd_micros ?? price?.input_price_usd_micros
    const outputMicros = channelModel.output_price_usd_micros ?? price?.output_price_usd_micros
    const billingMeter = channelModel.billing_meter ?? price?.billing_meter
    const unitMicros = channelModel.unit_price_usd_micros ?? price?.unit_price_usd_micros
    const cacheReadMicros =
      channelModel.cache_read_price_usd_micros ??
      price?.cache_read_price_usd_micros ??
      (inputMicros === undefined ? undefined : derivedCacheReadPrice(inputMicros))
    const cacheWriteMicros =
      channelModel.cache_write_price_usd_micros ?? price?.cache_write_price_usd_micros
    const billingEnabled = Boolean(channelModel.billing_enabled)
    const modelEnabled = Boolean(channelModel.enabled)
    const unitPrice =
      unitMicros !== undefined && unitMicros !== null
        ? formatUsdPerMillion(microUsdToUsd(unitMicros))
        : t('priceMissing')
    const inputPrice =
      inputMicros !== undefined && inputMicros !== null
        ? formatUsdPerMillion(microUsdToUsd(inputMicros))
        : t('priceMissing')
    const outputPrice =
      outputMicros !== undefined && outputMicros !== null
        ? formatUsdPerMillion(microUsdToUsd(outputMicros))
        : t('priceMissing')
    const cacheReadPrice =
      cacheReadMicros !== undefined && cacheReadMicros !== null
        ? formatUsdPerMillion(microUsdToUsd(cacheReadMicros))
        : t('priceMissing')
    const cacheWritePrice = hasConfiguredPrice
      ? cacheWriteMicros === undefined || cacheWriteMicros === null
        ? '$0'
        : formatUsdPerMillion(microUsdToUsd(cacheWriteMicros as number))
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
        billingMeter === 'image'
          ? '-'
          : price && billingMeter === 'token'
            ? `${cacheReadPrice} / ${cacheWritePrice}`
            : t('priceMissing'),
      price:
        billingMeter === 'image'
          ? `${unitPrice} / ${t('perImage')}`
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
      const [fetchedPrices, fetchedTemplates, fetchedProviderModels] = await Promise.all([
        getProviderPrices(),
        getPricingTemplates(),
        getProviderModels()
      ])
      prices.value = fetchedPrices
      templates.value = fetchedTemplates
      providerModels.value = fetchedProviderModels
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
  const record = providerModelByModel.value.get(priceKey(provider, model))
  const output = record ? capabilityValues(record.capabilities, ['modalities', 'output']) : []
  if (output.length > 0) return output

  const template = findPricingTemplate(templates.value, provider, model)
  if (!template || template.provider === provider) return output

  const referenceRecord = providerModelByModel.value.get(priceKey(template.provider, model))
  return referenceRecord
    ? capabilityValues(referenceRecord.capabilities, ['modalities', 'output'])
    : output
}

function canUseImageBilling(provider: string, model: string) {
  const output = modelOutputModalities(provider, model)
  return output.length === 1 && output[0] === 'image'
}

function defaultBillingMeterForModel(provider: string, model: string) {
  return canUseImageBilling(provider, model) ? 'image' : 'token'
}

function isBillingMeterLocked(provider: string, model: string) {
  return !canUseImageBilling(provider, model)
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
  if (form.billingMeter === 'image') return form.unitUsd > 0
  return (
    form.inputUsdPerMillion > 0 ||
    form.outputUsdPerMillion > 0 ||
    form.cacheReadUsdPerMillion > 0 ||
    (form.cacheWriteUsdPerMillion ?? 0) > 0
  )
}

function hasEnabledBillablePrice(price?: ProviderPrice, billingMeter?: BillingMeter | null) {
  if (!price?.enabled) return false
  if (billingMeter && price.billing_meter !== billingMeter) return false
  if (price.billing_meter === 'image') return (price.unit_price_usd_micros ?? 0) > 0
  return price.input_price_usd_micros > 0 || price.output_price_usd_micros > 0
}

function shouldSavePriceForm(form: ChannelPriceForm) {
  return form.hasPriceRecord || hasReferencePrice(form) || hasManualPriceInput(form)
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
    const savedBillingMeter =
      price?.billing_meter === 'image' && supportsImageBilling
        ? 'image'
        : price?.billing_meter === 'token'
          ? 'token'
          : null
    const billingMeter = savedBillingMeter ?? defaultBillingMeterForModel(row.provider, model)
    const inputPrice = price?.input_price_usd_micros ?? template?.input_price_usd_micros ?? 0
    const cacheWritePrice = template
      ? template.cache_write_price_usd_micros
      : (price?.cache_write_price_usd_micros ?? inputPrice)
    priceForms[key] = {
      provider: row.provider,
      model,
      billingMeter,
      inputUsdPerMillion: microUsdToUsd(inputPrice),
      outputUsdPerMillion: microUsdToUsd(
        price?.output_price_usd_micros ?? template?.output_price_usd_micros ?? 0
      ),
      cacheReadUsdPerMillion: microUsdToUsd(
        price?.cache_read_price_usd_micros ??
          template?.cache_read_price_usd_micros ??
          derivedCacheReadPrice(inputPrice)
      ),
      cacheWriteUsdPerMillion:
        cacheWritePrice === undefined || cacheWritePrice === null
          ? 0
          : microUsdToUsd(cacheWritePrice),
      unitUsd: microUsdToUsd(price?.unit_price_usd_micros ?? template?.unit_price_usd_micros ?? 0),
      enabled: hasEnabledBillablePrice(price, billingMeter) || Boolean(template),
      hasPrice: hasEnabledBillablePrice(price, billingMeter),
      hasPriceRecord: Boolean(price),
      billingMeterLocked: isBillingMeterLocked(row.provider, model),
      canUseImageBilling: supportsImageBilling,
      templateSource: template ? pricingTemplateSourceLabel(template, row.provider) : undefined
    }
  }
  priceDialogOpen.value = true
}

function hasReferencePrice(form: (typeof priceForms)[string]) {
  return Boolean(findApplicablePricingTemplate(form))
}

function referencePriceFallbackLabel(form: (typeof priceForms)[string]) {
  return form.hasPrice ? t('referencePriceNotSynced') : t('priceMissing')
}

function referencePriceSummary(form: (typeof priceForms)[string]) {
  const template = findApplicablePricingTemplate(form)
  if (!template) return ''
  if (template.billing_meter === 'image') {
    const unit = template.unit_price_usd_micros
      ? formatUsdPerMillion(microUsdToUsd(template.unit_price_usd_micros))
      : t('priceMissing')
    return `${t('billingMeterImageGeneration')} ${unit} / ${t('perImage')}`
  }
  const input = formatUsdPerMillion(microUsdToUsd(template.input_price_usd_micros))
  const output = formatUsdPerMillion(microUsdToUsd(template.output_price_usd_micros))
  const cacheRead = formatUsdPerMillion(
    microUsdToUsd(
      template.cache_read_price_usd_micros ?? derivedCacheReadPrice(template.input_price_usd_micros)
    )
  )
  const cacheWrite =
    template.cache_write_price_usd_micros === undefined ||
    template.cache_write_price_usd_micros === null
      ? '$0'
      : formatUsdPerMillion(microUsdToUsd(template.cache_write_price_usd_micros))
  return `Token ${input} / ${output}\nCache ${cacheRead} / ${cacheWrite}`
}

function pricingTemplateSourceLabel(template: PricingTemplate, provider: string) {
  if (template.source === 'models_dev') return ''
  const source = template.source.replace(/_/g, '.')
  return template.provider === provider ? source : `${template.provider} / ${source}`
}

function priceIconProvider(form: (typeof priceForms)[string]) {
  return findApplicablePricingTemplate(form)?.provider ?? form.provider
}

function fillReferencePrice(form: (typeof priceForms)[string]) {
  const template = findApplicablePricingTemplate(form)
  if (!template) return
  form.billingMeter = template.billing_meter
  form.inputUsdPerMillion = microUsdToUsd(template.input_price_usd_micros)
  form.outputUsdPerMillion = microUsdToUsd(template.output_price_usd_micros)
  form.cacheReadUsdPerMillion = microUsdToUsd(
    template.cache_read_price_usd_micros ?? derivedCacheReadPrice(template.input_price_usd_micros)
  )
  form.cacheWriteUsdPerMillion =
    template.cache_write_price_usd_micros === undefined ||
    template.cache_write_price_usd_micros === null
      ? 0
      : microUsdToUsd(template.cache_write_price_usd_micros)
  form.unitUsd = microUsdToUsd(template.unit_price_usd_micros ?? 0)
}

function cacheWritePricePayload(form: (typeof priceForms)[string]) {
  return form.cacheWriteUsdPerMillion === null ? null : usdToMicroUsd(form.cacheWriteUsdPerMillion)
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
    templates.value = await getPricingTemplates()
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
        const billingMeter = requireBillingMeter(form)
        await upsertProviderPrice({
          provider: form.provider,
          model: form.model,
          input_price_usd_micros: usdToMicroUsd(form.inputUsdPerMillion),
          output_price_usd_micros: usdToMicroUsd(form.outputUsdPerMillion),
          cache_read_price_usd_micros: usdToMicroUsd(form.cacheReadUsdPerMillion),
          cache_write_price_usd_micros: cacheWritePricePayload(form),
          billing_meter: billingMeter,
          unit_price_usd_micros: billingMeter === 'image' ? usdToMicroUsd(form.unitUsd) : null,
          enabled: form.enabled
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

  await withLoading(savingPrices, async () => {
    try {
      for (const form of targetForms) {
        fillReferencePrice(form)
        const billingMeter = requireBillingMeter(form)
        await upsertProviderPrice({
          provider: form.provider,
          model: form.model,
          input_price_usd_micros: usdToMicroUsd(form.inputUsdPerMillion),
          output_price_usd_micros: usdToMicroUsd(form.outputUsdPerMillion),
          cache_read_price_usd_micros: usdToMicroUsd(form.cacheReadUsdPerMillion),
          cache_write_price_usd_micros: cacheWritePricePayload(form),
          billing_meter: billingMeter,
          unit_price_usd_micros: billingMeter === 'image' ? usdToMicroUsd(form.unitUsd) : null,
          enabled: true
        })
      }
      ElMessage.success(t('referencePricesApplied'))
      await loadPricingData()
      await loadChannels()
      priceDialogOpen.value = false
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function submitChannel() {
  const channel = await submitChannelBase(syncCreateReferencePricesIfNeeded)
  if (!channel) return
  await loadPricingData()
  if (channelPriceStatus(channel).missing > 0) {
    openPriceDialog(channel)
  }
}

async function submitEditChannel() {
  const channel = await submitEditChannelBase()
  if (!channel) return
  await loadPricingData()
}

async function loadInitialData() {
  try {
    await Promise.all([loadChannels(), loadPricingData()])
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
          @click="openCreateDialog"
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
        <el-table-column prop="name" :label="t('name')" min-width="120">
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
        <el-table-column :label="t('modelPrices')" min-width="170">
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
          min-width="100"
          class-name="channel-key-count-column"
          label-class-name="channel-key-count-header"
        >
          <template #default="{ row }">
            <span class="channel-key-count">{{ channelCredentialSummary(row).label }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('probeTrend')"
          min-width="140"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <ChannelProbeTrendCell :channel="row" :latency-scale="probeTrendLatencyScale" />
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelStatus')"
          min-width="100"
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
          min-width="130"
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
        <el-table-column
          :label="t('actions')"
          width="130"
          fixed="right"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-tooltip :content="t('configurePrice')" placement="top" :show-after="600">
                <el-button
                  class="admin-action-button icon-only-action price-config-action"
                  :aria-label="t('configurePrice')"
                  :icon="Coin"
                  @click="openPriceDialog(row)"
                />
              </el-tooltip>
              <el-tooltip :content="t('edit')" placement="top" :show-after="600">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('edit')"
                  :icon="Edit"
                  @click="openEditDialog(row)"
                />
              </el-tooltip>
              <el-dropdown trigger="click" placement="bottom-end">
                <el-button
                  class="admin-action-button icon-only-action action-more-button"
                  :aria-label="t('moreActions')"
                  :icon="MoreFilled"
                  :loading="isChannelDiagnosing(row.id)"
                />
                <template #dropdown>
                  <el-dropdown-menu class="admin-row-action-menu">
                    <el-dropdown-item
                      :disabled="diagnosticInProgress"
                      @click="runChannelDiagnostic(row)"
                    >
                      <el-icon><Search /></el-icon>
                      <span>{{ t('fullDiagnoseChannel') }}</span>
                    </el-dropdown-item>
                    <el-dropdown-item
                      class="is-danger"
                      :disabled="deletingId === row.id"
                      @click="confirmDeleteChannel(row)"
                    >
                      <el-icon><Delete /></el-icon>
                      <span>{{ t('delete') }}</span>
                    </el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
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
      :base-url-readonly="isCreateBaseUrlReadonly"
      :fetching-models="fetchingModels"
      :submitting="creating"
      :models-input-placeholder="modelsInputPlaceholder()"
      :models-input-readonly="modelsInputReadonly()"
      :secret-placeholder="t('optionalUpstreamKey')"
      @fetch-models="fetchCreateModels"
      @select-provider="selectCreateProvider"
      @submit="submitChannel"
    />

    <ModelPickerDialog
      v-model:open="modelPickerDialogOpen"
      v-model:selected-models="selectedFetchedModels"
      :models="fetchedModels"
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
      :base-url-readonly="isEditBaseUrlReadonly"
      :fetching-models="fetchingModels"
      :submitting="updating"
      :models-input-placeholder="modelsInputPlaceholder()"
      :models-input-readonly="modelsInputReadonly()"
      :secret-placeholder="t('optionalEditUpstreamKey')"
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
      :reference-price-fallback-label="referencePriceFallbackLabel"
      :price-icon-provider="priceIconProvider"
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
  gap: 11px;
  max-width: 100%;
  min-width: 0;
  vertical-align: middle;
}

.channel-name-cell :deep(.provider-icon) {
  border-radius: 8px;
  height: 30px;
  width: 30px;
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

.channel-table {
  min-width: 0 !important;
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
  background: #eef7fd;
  border: 1px solid #cde9f8;
  border-radius: 999px 0 0 999px;
  color: #0f76b8;
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
