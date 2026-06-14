<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref } from 'vue'
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Coin,
  Delete,
  Edit,
  Loading,
  MoreFilled,
  Plus,
  PriceTag,
  Search,
  VideoPause,
  WarningFilled
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  getPricingTemplates,
  getProviderPrices,
  syncPricingTemplates,
  upsertProviderPrice
} from '../../api/prices'
import {
  streamChannelDiagnostic,
  updateChannel,
  type ChannelDiagnosticStreamEvent
} from '../../api/channels'
import ChannelFormDialog from '../../components/admin/channels/ChannelFormDialog.vue'
import ChannelPriceDialog, {
  type ChannelPriceForm
} from '../../components/admin/channels/ChannelPriceDialog.vue'
import AdminActionTooltip from '../../components/admin/AdminActionTooltip.vue'
import ModelPickerDialog from '../../components/admin/channels/ModelPickerDialog.vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useChannels } from '../../composables/useChannels'
import { useLocale } from '../../composables/useLocale'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type {
  Channel,
  ChannelDiagnosticReport,
  ChannelKey,
  ChannelProbeSample,
  DiagnosticStep,
  DiagnosticStatus,
  EndpointDiagnosticReport,
  PricingTemplate,
  ProviderPrice
} from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import {
  formatCompactDateTime,
  formatDurationMs,
  formatUsdPerMillion,
  microUsdToUsd,
  usdToMicroUsd
} from '../../utils/format'
import { splitCommaList } from '../../utils/channel'
import {
  derivedCacheReadPrice,
  findPricingTemplate,
  isProviderPriceConfigured,
  isProviderPriceReady,
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
const pricingLoading = ref(true)
const channelsLoaded = ref(false)
const priceDialogOpen = ref(false)
const savingPrices = ref(false)
const diagnosticDialogOpen = ref(false)
const diagnosticReport = ref<ChannelDiagnosticReport | null>(null)
const diagnosticError = ref('')
const diagnosticChannel = ref<Channel | null>(null)
const diagnosingChannelId = ref<number | null>(null)
const diagnosticLiveSteps = ref<
  Array<Extract<ChannelDiagnosticStreamEvent, { type: 'model_result' }>>
>([])
const diagnosticCurrentModel = ref('')
const diagnosticLiveListRef = ref<HTMLElement | null>(null)
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

const diagnosticInProgress = computed(() => diagnosingChannelId.value !== null)

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

const channelHasPagination = computed(
  () => filteredChannels.value.length > channelPageSize.value
)

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
  const models = row.endpoints.flatMap((endpoint) => endpoint.models)
  return Array.from(new Set(models.map((model) => model.trim()).filter(Boolean)))
}

function channelPriceStatus(row: Channel) {
  const models = channelModelList(row)
  if (models.length === 0) {
    return { missing: 0, total: 0, type: 'info' as const, label: '-' }
  }

  if (pricingLoading.value && prices.value.length === 0) {
    return { missing: 0, total: models.length, type: 'info' as const, label: '-' }
  }

  const missing = models.filter(
    (model) => !isProviderPriceConfigured(priceByModel.value.get(priceKey(row.provider, model)))
  ).length
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
  return channelModelList(row).map((model) => {
    const price = priceByModel.value.get(priceKey(row.provider, model))
    if (pricingLoading.value && prices.value.length === 0) {
      return {
        model,
        disabled: false,
        missing: false,
        inputPrice: '-',
        outputPrice: '-',
        cacheReadPrice: '-',
        cacheWritePrice: '-',
        price: '-'
      }
    }

    const inputPrice = price
      ? formatUsdPerMillion(microUsdToUsd(price.input_price_usd_micros))
      : t('priceMissing')
    const outputPrice = price
      ? formatUsdPerMillion(microUsdToUsd(price.output_price_usd_micros))
      : t('priceMissing')
    const cacheReadPrice = price
      ? formatUsdPerMillion(
          microUsdToUsd(
            price.cache_read_price_usd_micros ?? derivedCacheReadPrice(price.input_price_usd_micros)
          )
        )
      : t('priceMissing')
    const cacheWriteMicros = price?.cache_write_price_usd_micros
    const cacheWritePrice = price
      ? cacheWriteMicros === undefined || cacheWriteMicros === null
        ? '$0'
        : formatUsdPerMillion(microUsdToUsd(cacheWriteMicros as number))
      : t('priceMissing')
    const hasConfiguredPrice = isProviderPriceConfigured(price)
    const runtimeEnabled = isProviderPriceReady(price)
    return {
      model,
      disabled: Boolean(price && !price.enabled),
      missing: !hasConfiguredPrice,
      runtimeToggleDisabled: !hasConfiguredPrice || isRuntimeToggling(row.provider, model),
      runtimeEnabled,
      inputPrice,
      outputPrice,
      cacheReadPrice,
      cacheWritePrice,
      price: price ? `${inputPrice} / ${outputPrice}` : t('priceMissing')
    }
  })
}

function channelPricePreviewRows(row: Channel) {
  return channelPriceRows(row).slice(0, 3)
}

function channelPriceOverflowCount(row: Channel) {
  return Math.max(channelModelList(row).length - channelPricePreviewRows(row).length, 0)
}

function isRuntimeToggling(provider: string, model: string) {
  return togglingRuntimeKeys.has(priceKey(provider, model))
}

function isChannelToggling(channelId: number) {
  return togglingChannelIds.has(channelId)
}

function isChannelDiagnosing(channelId: number) {
  return diagnosingChannelId.value === channelId
}

function diagnosticStatusLabel(status: DiagnosticStatus) {
  const labels: Record<DiagnosticStatus, string> = {
    ok: t('diagnosticStatusOk'),
    warning: t('diagnosticStatusWarning'),
    failed: t('diagnosticStatusFailed'),
    skipped: t('diagnosticStatusSkipped')
  }
  return labels[status]
}

function diagnosticStatusType(status: DiagnosticStatus) {
  const types: Record<DiagnosticStatus, 'success' | 'warning' | 'danger' | 'info'> = {
    ok: 'success',
    warning: 'warning',
    failed: 'danger',
    skipped: 'info'
  }
  return types[status]
}

function diagnosticStepLabel(name: string) {
  if (name === 'models') return t('diagnosticStepModels')
  if (name === 'probe') return t('diagnosticStepProbe')
  if (name.startsWith('probe:')) return `${t('diagnosticStepProbe')} · ${name.slice(6)}`
  return name
}

async function scrollDiagnosticLiveListToBottom() {
  await nextTick()
  const list = diagnosticLiveListRef.value
  if (list) list.scrollTop = list.scrollHeight
}

function diagnosticModelsPreview(models: string[]) {
  if (models.length === 0) return t('diagnosticNoModels')
  return models.slice(0, 6).join(', ') + (models.length > 6 ? ` +${models.length - 6}` : '')
}

function diagnosticEndpointCount(report: ChannelDiagnosticReport) {
  return report.endpoints.length
}

function diagnosticKeyCount(report: ChannelDiagnosticReport) {
  return report.endpoints.reduce((count, endpoint) => count + endpoint.keys.length, 0)
}

function diagnosticAvailableKeyCount(report: ChannelDiagnosticReport) {
  return report.endpoints.reduce(
    (count, endpoint) => count + endpoint.keys.filter((key) => key.status === 'ok').length,
    0
  )
}

function diagnosticConfiguredModelCount(endpoint: EndpointDiagnosticReport) {
  return endpoint.configured_models.length
}

function diagnosticDiscoveredModelSummary(endpoint: EndpointDiagnosticReport) {
  const count = endpoint.discovered_models.length
  return count > 0 ? `${count}` : t('diagnosticNoModels')
}

function diagnosticStepMeta(step: DiagnosticStep) {
  return `${formatDurationMs(step.duration_ms)}${step.status_code ? ` · HTTP ${step.status_code}` : ''}`
}

function diagnosticEndpointTitle(endpoint: EndpointDiagnosticReport) {
  return `${endpoint.protocol.toUpperCase()} · ${endpoint.base_url}`
}

function latestProbeSample(row: Channel) {
  return row.probe_samples.length > 0 ? row.probe_samples[row.probe_samples.length - 1] : null
}

function probeTrendStats(row: Channel) {
  const samples = row.probe_samples
  const latencySamples = samples.filter((sample) => sample.latency_ms != null)
  const latencyValues = latencySamples.map((sample) => sample.latency_ms ?? 0)
  const latest = latestProbeSample(row)
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
    latest,
    avgLatency,
    minLatency,
    maxLatency
  }
}

function probeTrendPoints(row: Channel) {
  const samples = row.probe_samples.filter((sample) => sample.latency_ms != null)
  if (samples.length === 0) return ''
  const values = samples.map((sample) => sample.latency_ms ?? 0)
  const min = Math.min(...values)
  const max = Math.max(...values)
  const range = Math.max(max - min, 1)
  const width = 132
  const height = 34
  return values
    .map((value, index) => {
      const x = samples.length === 1 ? width : (index / (samples.length - 1)) * width
      const y = height - ((value - min) / range) * (height - 10) - 5
      return `${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
}

function probeTrendClass(row: Channel) {
  const latest = latestProbeSample(row)
  if (!latest) return 'is-empty'
  return latest.status === 'ok' ? 'is-ok' : 'is-failed'
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

function probeTrendSuccessLabel(row: Channel) {
  const stats = probeTrendStats(row)
  return stats.total === 0 ? '-' : `${stats.okCount}/${stats.total}`
}

function probeTrendAverageLabel(row: Channel) {
  const stats = probeTrendStats(row)
  return stats.avgLatency == null ? '-' : formatDurationMs(stats.avgLatency)
}

function probeTooltip(row: Channel) {
  const stats = probeTrendStats(row)
  const sample = stats.latest
  if (!sample) return t('probeNoDataHint')
  const time = formatCompactDateTime(sample.created_at)
  const model = sample.model || '-'
  const status = sample.status === 'ok' ? t('diagnosticStatusOk') : t('diagnosticStatusFailed')
  return [
    `${t('time')}: ${time}`,
    `${t('model')}: ${model}`,
    `${t('channelStatus')}: ${status}`,
    `${t('latency')}: ${probeLatencyLabel(sample)}`,
    `${t('probeSuccessRatio')}: ${stats.okCount}/${stats.total}`,
    stats.avgLatency != null ? `${t('probeAverageLatency')}: ${formatDurationMs(stats.avgLatency)}` : '',
    stats.minLatency != null && stats.maxLatency != null
      ? `${t('probeLatencyRange')}: ${formatDurationMs(stats.minLatency)} - ${formatDurationMs(stats.maxLatency)}`
      : '',
    sample.status_code ? `${t('probeStatusCode')}: ${sample.status_code}` : '',
    sample.error_summary ? `${t('error')}: ${sample.error_summary}` : ''
  ]
    .filter(Boolean)
    .join('\n')
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
  pricingLoading.value = true
  try {
    const [fetchedPrices, fetchedTemplates] = await Promise.all([
      getProviderPrices(),
      getPricingTemplates()
    ])
    prices.value = fetchedPrices
    templates.value = fetchedTemplates
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    pricingLoading.value = false
  }
}

function openPriceDialog(row: Channel) {
  for (const key of Object.keys(priceForms)) {
    delete priceForms[key]
  }

  for (const model of channelModelList(row)) {
    const key = priceKey(row.provider, model)
    const price = priceByModel.value.get(key)
    const template = findPricingTemplate(templates.value, row.provider, model)
    const inputPrice = price?.input_price_usd_micros ?? template?.input_price_usd_micros ?? 0
    const cacheWritePrice = template
      ? template.cache_write_price_usd_micros
      : (price?.cache_write_price_usd_micros ?? inputPrice)
    priceForms[key] = {
      provider: row.provider,
      model,
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
      enabled: price?.enabled ?? true,
      hasPrice: Boolean(price),
      templateSource: template ? pricingTemplateSourceLabel(template, row.provider) : undefined
    }
  }
  priceDialogOpen.value = true
}

function hasReferencePrice(form: (typeof priceForms)[string]) {
  return Boolean(findPricingTemplate(templates.value, form.provider, form.model))
}

function referencePriceFallbackLabel(form: (typeof priceForms)[string]) {
  return form.hasPrice ? t('referencePriceNotSynced') : t('priceMissing')
}

function referencePriceSummary(form: (typeof priceForms)[string]) {
  const template = findPricingTemplate(templates.value, form.provider, form.model)
  if (!template) return ''
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
  const source = template.source.replace(/_/g, '.')
  return template.provider === provider ? source : `${template.provider} / ${source}`
}

function priceIconProvider(form: (typeof priceForms)[string]) {
  return findPricingTemplate(templates.value, form.provider, form.model)?.provider ?? form.provider
}

function fillReferencePrice(form: (typeof priceForms)[string]) {
  const template = findPricingTemplate(templates.value, form.provider, form.model)
  if (!template) return
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
}

function cacheWritePricePayload(form: (typeof priceForms)[string]) {
  return form.cacheWriteUsdPerMillion === null ? null : usdToMicroUsd(form.cacheWriteUsdPerMillion)
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
  savingPrices.value = true
  try {
    for (const form of Object.values(priceForms)) {
      await upsertProviderPrice({
        provider: form.provider,
        model: form.model,
        input_price_usd_micros: usdToMicroUsd(form.inputUsdPerMillion),
        output_price_usd_micros: usdToMicroUsd(form.outputUsdPerMillion),
        cache_read_price_usd_micros: usdToMicroUsd(form.cacheReadUsdPerMillion),
        cache_write_price_usd_micros: cacheWritePricePayload(form),
        enabled: form.enabled
      })
    }
    ElMessage.success(t('priceSaved'))
    await loadPricingData()
    await loadChannels()
    priceDialogOpen.value = false
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    savingPrices.value = false
  }
}

async function toggleChannelModelRuntime(provider: string, model: string, enabled: boolean) {
  const price = priceByModel.value.get(priceKey(provider, model))
  if (!price || !isProviderPriceConfigured(price) || isRuntimeToggling(provider, model)) return

  togglingRuntimeKeys.add(priceKey(provider, model))
  try {
    await upsertProviderPrice({
      provider,
      model,
      input_price_usd_micros: price.input_price_usd_micros,
      output_price_usd_micros: price.output_price_usd_micros,
      cache_read_price_usd_micros: price.cache_read_price_usd_micros ?? null,
      cache_write_price_usd_micros: price.cache_write_price_usd_micros ?? null,
      enabled
    })
    await loadPricingData()
    await loadChannels()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    togglingRuntimeKeys.remove(priceKey(provider, model))
  }
}

async function toggleChannelRuntime(row: Channel, enabled: boolean) {
  if (row.enabled === enabled || isChannelToggling(row.id)) return

  togglingChannelIds.add(row.id)
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
  } finally {
    togglingChannelIds.remove(row.id)
  }
}

async function runChannelDiagnostic(row: Channel) {
  if (diagnosticInProgress.value) return

  diagnosticChannel.value = row
  diagnosticReport.value = null
  diagnosticError.value = ''
  diagnosticLiveSteps.value = []
  diagnosticCurrentModel.value = ''
  diagnosticDialogOpen.value = true
  diagnosingChannelId.value = row.id
  try {
    diagnosticReport.value = await streamChannelDiagnostic(row.id, (event) => {
      if (event.type === 'model_started') {
        diagnosticCurrentModel.value = event.model
      }
      if (event.type === 'model_result') {
        diagnosticLiveSteps.value.push(event)
        diagnosticCurrentModel.value = ''
        void scrollDiagnosticLiveListToBottom()
      }
      if (event.type === 'finished') {
        diagnosticReport.value = event.report
      }
    })
    await loadChannels()
  } catch (err) {
    diagnosticError.value = readError(err)
  } finally {
    diagnosingChannelId.value = null
  }
}

async function applyReferencePrices() {
  const targetForms = Object.values(priceForms).filter(hasReferencePrice)
  if (targetForms.length === 0) {
    ElMessage.warning(t('noReferencePrices'))
    return
  }

  savingPrices.value = true
  try {
    for (const form of targetForms) {
      fillReferencePrice(form)
      await upsertProviderPrice({
        provider: form.provider,
        model: form.model,
        input_price_usd_micros: usdToMicroUsd(form.inputUsdPerMillion),
        output_price_usd_micros: usdToMicroUsd(form.outputUsdPerMillion),
        cache_read_price_usd_micros: usdToMicroUsd(form.cacheReadUsdPerMillion),
        cache_write_price_usd_micros: cacheWritePricePayload(form),
        enabled: true
      })
    }
    ElMessage.success(t('referencePricesApplied'))
    await loadPricingData()
    await loadChannels()
    priceDialogOpen.value = false
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    savingPrices.value = false
  }
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
        v-loading="loading"
        class="admin-table service-table channel-table"
        :data="paginatedChannels"
        :row-class-name="channelRowClassName"
        row-key="id"
        stripe
      >
        <el-table-column type="expand" width="44">
          <template #default="{ row }">
            <div class="channel-expand-panel">
              <div class="channel-expand-head">
                <div>
                  <strong>{{ t('modelPriceDetails') }}</strong>
                  <span>{{ t('inputOutputPriceHint') }}</span>
                </div>
                <el-button
                  class="admin-action-button expand-price-action"
                  :icon="PriceTag"
                  @click="openPriceDialog(row)"
                >
                  {{ t('configurePrice') }}
                </el-button>
              </div>
              <div class="channel-expand-price-table">
                <div class="channel-expand-price-row is-head">
                  <span>{{ t('modelName') }}</span>
                  <span class="channel-head-label">{{ t('inputPrice') }}</span>
                  <span class="channel-head-label">{{ t('outputPrice') }}</span>
                  <span class="channel-head-label">{{ t('cacheReadPrice') }}</span>
                  <span class="channel-head-label">{{ t('cacheWritePrice') }}</span>
                  <span>{{ t('priceStatus') }}</span>
                  <span>{{ t('channelStatus') }}</span>
                </div>
                <div
                  v-for="item in channelPriceRows(row)"
                  :key="item.model"
                  class="channel-expand-price-row"
                  :class="{ 'is-missing': item.missing, 'is-disabled': item.disabled }"
                >
                  <span class="channel-price-model">{{ item.model }}</span>
                  <span class="channel-detail-price">{{ item.inputPrice }}</span>
                  <span class="channel-detail-price">{{ item.outputPrice }}</span>
                  <span class="channel-detail-price">{{ item.cacheReadPrice }}</span>
                  <span class="channel-detail-price" :aria-label="item.cacheWritePrice">
                    {{ item.cacheWritePrice }}
                  </span>
                  <span
                    class="channel-detail-status"
                    :class="{ 'is-missing': item.missing }"
                    :aria-label="item.missing ? t('priceMissing') : t('priceReady')"
                  >
                    <el-icon>
                      <WarningFilled v-if="item.missing" />
                      <CircleCheckFilled v-else />
                    </el-icon>
                  </span>
                  <span
                    class="channel-detail-runtime-switch"
                    :aria-label="item.runtimeEnabled ? t('enabled') : t('disabled')"
                  >
                    <el-switch
                      :model-value="item.runtimeEnabled"
                      :disabled="item.runtimeToggleDisabled"
                      size="small"
                      @change="toggleChannelModelRuntime(row.provider, item.model, Boolean($event))"
                    />
                  </span>
                </div>
              </div>
            </div>
          </template>
        </el-table-column>
        <el-table-column prop="name" :label="t('name')" min-width="160">
          <template #default="{ row }">
            <span class="channel-name-cell">
              <ProviderIcon :provider="row.provider" />
              <span class="channel-name-stack">
                <span class="channel-name-text">{{ row.name }}</span>
                <span class="channel-provider-text">{{ row.provider }}</span>
              </span>
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('modelPrices')" min-width="420">
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
                <span
                  v-for="item in channelPricePreviewRows(row)"
                  :key="item.model"
                  class="channel-price-item"
                  :class="{ 'is-missing': item.missing, 'is-disabled': item.disabled }"
                >
                  <span class="channel-price-model">{{ item.model }}</span>
                  <span class="channel-price-value">{{ item.price }}</span>
                </span>
                <span v-if="channelPriceOverflowCount(row) > 0" class="channel-price-more">
                  +{{ channelPriceOverflowCount(row) }}
                </span>
              </div>
            </div>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelKeyCountShort')"
          min-width="150"
          class-name="channel-key-count-column"
          label-class-name="channel-key-count-header"
        >
          <template #default="{ row }">
            <span class="channel-key-count">{{ channelCredentialSummary(row).label }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('probeTrend')" min-width="220" align="center" header-align="center">
          <template #default="{ row }">
            <el-tooltip
              placement="top"
              effect="light"
              popper-class="probe-trend-tooltip"
              :show-after="180"
            >
              <template #content>
                <div class="probe-tooltip-content">
                  <div class="probe-tooltip-title">{{ latestProbeSample(row)?.model || '-' }}</div>
                  <div class="probe-tooltip-grid">
                    <span>{{ t('time') }}</span>
                    <strong>{{
                      latestProbeSample(row)
                        ? formatCompactDateTime(latestProbeSample(row)?.created_at)
                        : '-'
                    }}</strong>
                    <span>{{ t('channelStatus') }}</span>
                    <strong>{{ probeSampleStatusLabel(latestProbeSample(row)) }}</strong>
                    <span>{{ t('latency') }}</span>
                    <strong>{{ probeLatencyLabel(latestProbeSample(row)) }}</strong>
                    <span>{{ t('probeSuccessRatio') }}</span>
                    <strong>{{ probeTrendSuccessLabel(row) }}</strong>
                    <span>{{ t('probeAverageLatency') }}</span>
                    <strong>{{ probeTrendAverageLabel(row) }}</strong>
                    <template
                      v-if="
                        probeTrendStats(row).minLatency != null &&
                        probeTrendStats(row).maxLatency != null
                      "
                    >
                      <span>{{ t('probeLatencyRange') }}</span>
                      <strong>
                        {{ formatDurationMs(probeTrendStats(row).minLatency) }} -
                        {{ formatDurationMs(probeTrendStats(row).maxLatency) }}
                      </strong>
                    </template>
                    <template v-if="latestProbeSample(row)?.status_code">
                      <span>{{ t('probeStatusCode') }}</span>
                      <strong>{{ latestProbeSample(row)?.status_code }}</strong>
                    </template>
                    <template v-if="latestProbeSample(row)?.error_summary">
                      <span>{{ t('error') }}</span>
                      <strong>{{ latestProbeSample(row)?.error_summary }}</strong>
                    </template>
                  </div>
                </div>
              </template>
              <div
                class="probe-trend-cell"
                :class="probeTrendClass(row)"
                :aria-label="probeTooltip(row)"
              >
                <svg class="probe-trend-chart" viewBox="0 0 132 34" aria-hidden="true">
                  <line
                    x1="0"
                    y1="29"
                    x2="132"
                    y2="29"
                    class="probe-trend-baseline"
                    stroke-linecap="round"
                  />
                  <polyline
                    v-if="probeTrendPoints(row)"
                    :points="probeTrendPoints(row)"
                    fill="none"
                    class="probe-trend-line"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2.4"
                  />
                  <line
                    v-else
                    x1="14"
                    y1="17"
                    x2="118"
                    y2="17"
                    class="probe-trend-line"
                    stroke-width="2.4"
                    stroke-linecap="round"
                  />
                </svg>
                <div class="probe-trend-foot">
                  <span>{{ t('probeAverageLatency') }}</span>
                  <strong>{{ probeTrendAverageLabel(row) }}</strong>
                </div>
              </div>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('channelStatus')"
          min-width="190"
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
        <el-table-column :label="t('actions')" width="150" fixed="right" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <AdminActionTooltip :content="t('configurePrice')">
                <el-button
                  class="admin-action-button icon-only-action price-config-action"
                  :aria-label="t('configurePrice')"
                  :icon="Coin"
                  @click="openPriceDialog(row)"
                />
              </AdminActionTooltip>
              <AdminActionTooltip :content="t('edit')">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('edit')"
                  :icon="Edit"
                  @click="openEditDialog(row)"
                />
              </AdminActionTooltip>
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
            <el-option
              v-for="s in channelPageSizes"
              :key="s"
              :value="s"
              :label="String(s)"
            />
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

    <el-dialog
      v-model="diagnosticDialogOpen"
      class="channel-dialog diagnostic-dialog"
      :title="t('channelDiagnosticReport')"
      width="760px"
      :close-on-click-modal="!diagnosticInProgress"
      :close-on-press-escape="!diagnosticInProgress"
    >
      <div v-if="diagnosticInProgress && diagnosticChannel" class="diagnostic-running">
        <div class="diagnostic-running-icon">
          <el-icon><Loading /></el-icon>
        </div>
        <div class="diagnostic-running-copy">
          <strong>{{ t('diagnosticRunningTitle') }}</strong>
          <span>{{ diagnosticChannel.name }} · {{ diagnosticChannel.provider }}</span>
          <p>{{ t('diagnosticRunningHint') }}</p>
        </div>
        <div v-if="diagnosticCurrentModel" class="diagnostic-current-model">
          <span>{{ t('diagnosticCurrentModel') }}</span>
          <strong>{{ diagnosticCurrentModel }}</strong>
        </div>
        <div ref="diagnosticLiveListRef" class="diagnostic-live-list">
          <div v-if="diagnosticLiveSteps.length === 0" class="diagnostic-live-empty">
            {{ t('diagnosticWaitingFirstResult') }}
          </div>
          <div
            v-for="event in diagnosticLiveSteps"
            :key="`${event.endpoint_id}-${event.key_id ?? event.key_name}-${event.model}`"
            class="diagnostic-step"
            :class="`is-${event.step.status}`"
          >
            <span class="diagnostic-step-dot"></span>
            <div class="diagnostic-step-copy">
              <strong>{{ diagnosticStepLabel(event.step.name) }}</strong>
              <span>{{ event.step.message }}</span>
            </div>
            <span class="diagnostic-step-meta">{{ diagnosticStepMeta(event.step) }}</span>
          </div>
        </div>
      </div>

      <div v-else-if="diagnosticError" class="diagnostic-error">
        <el-alert :title="t('diagnosticFailedTitle')" :description="diagnosticError" type="error" show-icon />
        <el-button
          v-if="diagnosticChannel"
          class="admin-action-button"
          type="primary"
          @click="runChannelDiagnostic(diagnosticChannel)"
        >
          {{ t('retry') }}
        </el-button>
      </div>

      <div v-else-if="diagnosticReport" class="diagnostic-report">
        <div class="diagnostic-result-card" :class="`is-${diagnosticReport.status}`">
          <div class="diagnostic-result-main">
            <span>{{ t('diagnosticResultOverview') }}</span>
            <strong>{{ diagnosticReport.summary }}</strong>
            <small>{{ diagnosticReport.channel_name }} · {{ diagnosticReport.provider }}</small>
          </div>
          <el-tag :type="diagnosticStatusType(diagnosticReport.status)" effect="light" round>
            {{ diagnosticStatusLabel(diagnosticReport.status) }}
          </el-tag>
        </div>

        <div class="diagnostic-stats">
          <div class="diagnostic-stat">
            <span>{{ t('latency') }}</span>
            <strong>{{ formatDurationMs(diagnosticReport.duration_ms) }}</strong>
          </div>
          <div class="diagnostic-stat">
            <span>{{ t('diagnosticTestedEndpoints') }}</span>
            <strong>{{ diagnosticEndpointCount(diagnosticReport) }}</strong>
          </div>
          <div class="diagnostic-stat">
            <span>{{ t('diagnosticTestedKeys') }}</span>
            <strong>{{ diagnosticKeyCount(diagnosticReport) }}</strong>
          </div>
          <div class="diagnostic-stat">
            <span>{{ t('diagnosticAvailableKeys') }}</span>
            <strong>{{ diagnosticAvailableKeyCount(diagnosticReport) }}</strong>
          </div>
        </div>

        <div class="diagnostic-section">
          <div class="diagnostic-section-title">
            <strong>{{ t('diagnosticEndpointOverview') }}</strong>
          </div>
          <div
            v-for="endpoint in diagnosticReport.endpoints"
            :key="endpoint.endpoint_id"
            class="diagnostic-endpoint-card"
          >
            <div class="diagnostic-endpoint-head">
              <div>
                <strong>{{ diagnosticEndpointTitle(endpoint) }}</strong>
                <span>{{ endpoint.summary }}</span>
              </div>
              <el-tag :type="diagnosticStatusType(endpoint.status)" size="small" effect="light">
                {{ diagnosticStatusLabel(endpoint.status) }}
              </el-tag>
            </div>
            <div class="diagnostic-endpoint-facts">
              <span>
                {{ t('diagnosticConfiguredModels') }}
                <strong>{{ diagnosticConfiguredModelCount(endpoint) }}</strong>
              </span>
              <span>
                {{ t('diagnosticDiscoveredModels') }}
                <strong>{{ diagnosticDiscoveredModelSummary(endpoint) }}</strong>
              </span>
              <span v-if="endpoint.missing_configured_models.length" class="is-warning">
                {{ t('diagnosticMissingModels') }}
                <strong>{{ diagnosticModelsPreview(endpoint.missing_configured_models) }}</strong>
              </span>
            </div>
            <div v-if="endpoint.discovered_models.length" class="diagnostic-model-preview">
              {{ diagnosticModelsPreview(endpoint.discovered_models) }}
            </div>
          </div>
        </div>

        <div class="diagnostic-section">
          <div class="diagnostic-section-title">
            <strong>{{ t('diagnosticKeyChecks') }}</strong>
          </div>
          <div
            v-for="endpoint in diagnosticReport.endpoints"
            :key="`keys-${endpoint.endpoint_id}`"
            class="diagnostic-key-group"
          >
            <div
              v-for="key in endpoint.keys"
              :key="`${endpoint.endpoint_id}-${key.key_id ?? key.key_name}`"
              class="diagnostic-key-item"
            >
              <div class="diagnostic-key-head">
                <div>
                  <strong>{{ key.key_name }}</strong>
                  <span v-if="key.key_prefix">{{ key.key_prefix }}</span>
                  <span v-else>{{ endpoint.protocol.toUpperCase() }}</span>
                </div>
                <el-tag :type="diagnosticStatusType(key.status)" size="small" effect="light">
                  {{ diagnosticStatusLabel(key.status) }}
                </el-tag>
              </div>
              <p>{{ key.summary }}</p>
              <div class="diagnostic-step-list">
                <div
                  v-for="step in key.steps"
                  :key="`${key.key_id ?? key.key_name}-${step.name}`"
                  class="diagnostic-step"
                  :class="`is-${step.status}`"
                >
                  <span class="diagnostic-step-dot"></span>
                  <div class="diagnostic-step-copy">
                    <strong>{{ diagnosticStepLabel(step.name) }}</strong>
                    <span>{{ step.message }}</span>
                  </div>
                  <span class="diagnostic-step-meta">{{ diagnosticStepMeta(step) }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </el-dialog>
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
  grid-template-columns: 180px minmax(260px, 1fr) 140px 160px;
  height: 48px;
  min-width: 1118px;
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
  grid-template-columns: 180px minmax(260px, 1fr);
  height: 82px;
  min-width: 1118px;
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

.channel-table :deep(.el-table__expanded-cell) {
  background: #f6f9fc;
  height: auto !important;
  padding: 0 !important;
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

.channel-table :deep(.el-table__expand-icon--expanded .el-icon) {
  transform: rotate(90deg);
}

.channel-table :deep(.channel-key-count-header .cell) {
  white-space: nowrap;
}

.channel-table :deep(.el-table__body .cell) {
  align-items: center;
  display: flex;
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
  display: inline-flex;
  gap: 0;
  inline-size: fit-content;
  min-width: 0;
  overflow: hidden;
  width: fit-content;
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
  background: #f1f5f9;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  color: #64748b;
  display: inline-flex;
  font-size: 12px;
  font-weight: 720;
  min-height: 24px;
  padding: 0 9px;
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

.probe-trend-cell {
  color: #17a169;
  display: inline-grid;
  gap: 3px;
  min-width: 156px;
  padding: 4px 0;
  text-align: left;
}

.probe-trend-cell.is-failed {
  color: #dc2626;
}

.probe-trend-cell.is-empty {
  color: #94a3b8;
}

.probe-trend-foot {
  align-items: center;
  display: flex;
  gap: 4px;
  justify-content: flex-start;
  min-width: 0;
}

.probe-trend-foot strong {
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.probe-trend-chart {
  color: inherit;
  display: block;
  height: 34px;
  width: 132px;
}

.probe-trend-baseline {
  stroke: #e5e7eb;
  stroke-width: 1;
}

.probe-trend-line {
  stroke: currentColor;
}

.probe-trend-foot {
  color: #667085;
  font-size: 11px;
  font-weight: 620;
  line-height: 1.1;
}

.probe-trend-foot strong {
  color: #344054;
  font-weight: 760;
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

.probe-tooltip-title {
  color: #111827;
  font-size: 13px;
  font-weight: 760;
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

.channel-expand-head span {
  color: #86909c;
  font-size: 12px;
  font-weight: 560;
  line-height: 1.2;
}

.expand-price-action.el-button {
  --el-button-bg-color: var(--brand-blue);
  --el-button-border-color: var(--brand-blue);
  --el-button-text-color: #ffffff;
  --el-button-hover-bg-color: var(--brand-blue-hover);
  --el-button-hover-border-color: var(--brand-blue-hover);
  --el-button-hover-text-color: #ffffff;
  box-shadow: none;
}

.expand-price-action.el-button:not(.is-disabled):hover {
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
  display: grid;
  gap: 10px;
  grid-template-columns:
    minmax(180px, 1.25fr)
    minmax(132px, 0.78fr)
    minmax(132px, 0.78fr)
    minmax(150px, 0.9fr)
    minmax(164px, 0.92fr)
    84px
    84px;
  min-height: 46px;
  min-width: 912px;
  padding: 0 16px;
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
.channel-expand-price-row.is-head span:nth-child(4),
.channel-expand-price-row.is-head span:nth-child(5),
.channel-detail-price {
  text-align: right;
}

.channel-expand-price-row.is-head span:nth-child(6) {
  padding-left: 20px;
  text-align: center;
}

.channel-expand-price-row.is-head span:nth-child(7) {
  text-align: center;
}

.channel-expand-price-row.is-missing {
  background: #fffaf3;
}

.channel-expand-price-row.is-disabled:not(.is-missing) {
  background: #f8fafc;
}

.channel-expand-price-row.is-missing .channel-price-model {
  color: #c2410c;
}

.channel-expand-price-row.is-disabled:not(.is-missing) .channel-price-model,
.channel-expand-price-row.is-disabled:not(.is-missing) .channel-detail-price,
.channel-expand-price-row.is-disabled:not(.is-missing) .channel-detail-status,
.channel-expand-price-row.is-disabled:not(.is-missing) .channel-detail-runtime-switch {
  color: #94a3b8;
}

.channel-expand-price-row.is-disabled:not(.is-missing) .channel-detail-status .el-icon {
  background: #cbd5e1;
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
  font-size: 13px;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 400;
  justify-content: flex-end;
  white-space: nowrap;
}

.channel-detail-status {
  align-items: center;
  display: inline-flex;
  justify-content: center;
  padding-left: 20px;
}

.channel-detail-status .el-icon {
  align-items: center;
  background: #22c55e;
  border-radius: 999px;
  color: #ffffff;
  display: inline-flex;
  font-size: 13px;
  height: 22px;
  justify-content: center;
  width: 22px;
}

.channel-detail-status.is-missing .el-icon {
  background: #f97316;
}

.channel-detail-runtime-switch {
  align-items: center;
  display: inline-flex;
  justify-content: center;
}

.channel-detail-runtime-switch :deep(.el-switch) {
  --el-switch-on-color: #22c55e;
  --el-switch-off-color: #94a3b8;
}

.channel-expand-price-row.is-missing .channel-detail-price,
.channel-expand-price-row.is-missing .channel-detail-status {
  color: #c2410c;
}

.channel-empty-state {
  padding: 30px 0 34px;
}

.diagnostic-report {
  display: grid;
  gap: 16px;
}

.diagnostic-running,
.diagnostic-error {
  display: grid;
  gap: 14px;
  min-height: 240px;
  place-items: center;
  text-align: center;
}

.diagnostic-running-copy {
  display: grid;
  gap: 6px;
  justify-items: center;
  max-width: 520px;
}

.diagnostic-running-copy strong {
  color: #1d2129;
  font-size: 16px;
  font-weight: 680;
}

.diagnostic-running-copy span {
  color: #86909c;
  font-size: 13px;
}

.diagnostic-running-copy p {
  color: #4e5969;
  line-height: 1.55;
  margin: 0;
}

.diagnostic-running-copy small {
  background: #fff7ed;
  border: 1px solid #fed7aa;
  border-radius: 999px;
  color: #c2410c;
  font-size: 12px;
  font-weight: 720;
  line-height: 1;
  padding: 6px 10px;
}

.diagnostic-current-model {
  align-items: center;
  background: #fff7ed;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  display: flex;
  gap: 8px;
  max-width: min(100%, 620px);
  padding: 10px 12px;
}

.diagnostic-current-model span {
  color: #c2410c;
  font-size: 12px;
  font-weight: 720;
  white-space: nowrap;
}

.diagnostic-current-model strong {
  color: #1d2129;
  font-size: 13px;
  font-weight: 760;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.diagnostic-live-list {
  display: grid;
  gap: 8px;
  max-height: 280px;
  overflow: auto;
  padding: 2px;
  width: min(100%, 620px);
}

.diagnostic-live-empty {
  background: #f8fafc;
  border: 1px dashed #d8e0ea;
  border-radius: 8px;
  color: #667085;
  font-size: 13px;
  font-weight: 620;
  padding: 12px;
}

.diagnostic-running-icon {
  align-items: center;
  background: #fff7ed;
  border-radius: 999px;
  color: #c2410c;
  display: inline-flex;
  font-size: 22px;
  height: 56px;
  justify-content: center;
  width: 56px;
}

.diagnostic-running-icon :deep(.el-icon) {
  animation: diagnostic-spin 1s linear infinite;
}

@keyframes diagnostic-spin {
  to {
    transform: rotate(360deg);
  }
}

.diagnostic-result-card {
  align-items: start;
  background: #f7fbf8;
  border: 1px solid #d8f0d1;
  border-radius: 8px;
  display: flex;
  gap: 14px;
  justify-content: space-between;
  padding: 14px 16px;
}

.diagnostic-result-card.is-warning {
  background: #fffaf0;
  border-color: #fdecc8;
}

.diagnostic-result-card.is-failed {
  background: #fff7f7;
  border-color: #ffd6d6;
}

.diagnostic-result-main {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.diagnostic-result-main span,
.diagnostic-section-title span,
.diagnostic-stat span,
.diagnostic-key-head span,
.diagnostic-step-copy span,
.diagnostic-step-meta,
.diagnostic-endpoint-head span,
.diagnostic-model-preview,
.diagnostic-endpoint-facts {
  color: #86909c;
  font-size: 12px;
}

.diagnostic-result-main strong {
  color: #1d2129;
  font-size: 16px;
  font-weight: 760;
  line-height: 1.35;
}

.diagnostic-result-main small {
  color: #667085;
  font-size: 12px;
  font-weight: 620;
}

.diagnostic-stats {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(4, minmax(0, 1fr));
}

.diagnostic-stat {
  background: #f8fafc;
  border: 1px solid #edf1f6;
  border-radius: 8px;
  display: grid;
  gap: 4px;
  padding: 10px 12px;
}

.diagnostic-stat strong {
  color: #1d2129;
  font-size: 16px;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 760;
}

.diagnostic-section {
  display: grid;
  gap: 10px;
}

.diagnostic-section-title {
  align-items: center;
  display: flex;
  justify-content: space-between;
}

.diagnostic-section-title strong {
  color: #344054;
  font-size: 14px;
  font-weight: 760;
}

.diagnostic-endpoint-card,
.diagnostic-key-item {
  border: 1px solid #e6edf5;
  border-radius: 8px;
  display: grid;
  gap: 12px;
  padding: 13px 14px;
}

.diagnostic-endpoint-head,
.diagnostic-key-head {
  align-items: start;
  display: flex;
  gap: 12px;
  justify-content: space-between;
}

.diagnostic-endpoint-head div,
.diagnostic-key-head div {
  display: grid;
  gap: 4px;
  min-width: 0;
}

.diagnostic-endpoint-head strong,
.diagnostic-key-head strong {
  color: #1d2129;
  font-size: 14px;
  font-weight: 760;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.diagnostic-endpoint-facts {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px 14px;
}

.diagnostic-endpoint-facts strong {
  color: #344054;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 760;
  margin-left: 4px;
}

.diagnostic-endpoint-facts .is-warning,
.diagnostic-endpoint-facts .is-warning strong {
  color: #c2410c;
}

.diagnostic-model-preview {
  background: #f8fafc;
  border-radius: 8px;
  line-height: 1.45;
  overflow-wrap: anywhere;
  padding: 9px 10px;
}

.diagnostic-key-group {
  display: grid;
  gap: 10px;
}

.diagnostic-key-item p {
  color: #4e5969;
  line-height: 1.45;
  margin: 0;
}

.diagnostic-step-list {
  display: grid;
  gap: 8px;
}

.diagnostic-step {
  align-items: center;
  background: #fbfcff;
  border-radius: 8px;
  display: grid;
  gap: 10px;
  grid-template-columns: 10px minmax(0, 1fr) auto;
  padding: 9px 10px;
}

.diagnostic-step-dot {
  background: #22c55e;
  border-radius: 999px;
  height: 8px;
  width: 8px;
}

.diagnostic-step.is-warning .diagnostic-step-dot {
  background: #eab308;
}

.diagnostic-step.is-failed .diagnostic-step-dot {
  background: #ef4444;
}

.diagnostic-step.is-skipped .diagnostic-step-dot {
  background: #94a3b8;
}

.diagnostic-step-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.diagnostic-step-copy strong {
  color: #1d2129;
  font-size: 13px;
  font-weight: 720;
}

.diagnostic-step-copy span {
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.diagnostic-step-meta {
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
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

  .diagnostic-stats {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .diagnostic-step {
    align-items: start;
    grid-template-columns: 10px minmax(0, 1fr);
  }

  .diagnostic-step-meta {
    grid-column: 2;
  }

  .diagnostic-result-card,
  .diagnostic-endpoint-head,
  .diagnostic-key-head {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
