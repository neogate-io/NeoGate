<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  CircleCheckFilled,
  Coin,
  Delete,
  Edit,
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
import ChannelFormDialog from '../../components/admin/channels/ChannelFormDialog.vue'
import ChannelPriceDialog, {
  type ChannelPriceForm
} from '../../components/admin/channels/ChannelPriceDialog.vue'
import ModelPickerDialog from '../../components/admin/channels/ModelPickerDialog.vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useChannels } from '../../composables/useChannels'
import { useLocale } from '../../composables/useLocale'
import type { Channel, ChannelKey, PricingTemplate, ProviderPrice } from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { formatUsdPerMillion, microUsdToUsd, usdToMicroUsd } from '../../utils/format'
import { splitCommaList } from '../../utils/channel'
import { findPricingTemplate, priceKey } from '../../utils/pricing'

const { t } = useLocale()

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
  editingChannel,
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
  confirmDeleteChannel
} = useChannels(t)

const prices = ref<ProviderPrice[]>([])
const templates = ref<PricingTemplate[]>([])
const priceDialogOpen = ref(false)
const savingPrices = ref(false)
const channelSearch = ref('')
const channelStatusFilter = ref<'all' | 'normal' | 'attention' | 'disabled'>('all')
const priceForms = reactive<Record<string, ChannelPriceForm>>({})

const priceByModel = computed(
  () => new Map(prices.value.map((price) => [priceKey(price.provider, price.model), price]))
)

const filteredChannels = computed(() => {
  const keyword = channelSearch.value.trim().toLowerCase()
  return channels.value.filter((channel) => {
    const runtimeStatus = channelRuntimeSummary(channel).status
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

function channelModelList(row: Channel) {
  const models = row.endpoints.flatMap((endpoint) => endpoint.models)
  return Array.from(new Set(models.map((model) => model.trim()).filter(Boolean)))
}

function derivedCacheReadPrice(inputPrice: number) {
  return Math.round(inputPrice / 10)
}

function channelPriceStatus(row: Channel) {
  const models = channelModelList(row)
  if (models.length === 0) {
    return { missing: 0, total: 0, type: 'info' as const, label: '-' }
  }

  const missing = models.filter(
    (model) => !priceByModel.value.get(priceKey(row.provider, model))?.enabled
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
    const hasEnabledPrice = Boolean(price?.enabled)
    return {
      model,
      disabled: Boolean(price && !price.enabled),
      missing: !hasEnabledPrice,
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

function channelHealthSummary(row: Channel) {
  const endpoints = row.endpoints
  if (endpoints.length === 0) {
    return { label: '-', type: 'info' as const, title: '' }
  }

  const healthyCount = endpoints.filter((endpoint) => endpoint.healthy).length
  const errorTitle = endpoints
    .filter((endpoint) => !endpoint.healthy && endpoint.last_error)
    .map((endpoint) => endpoint.last_error)
    .join('\n')

  if (healthyCount === endpoints.length) {
    return { label: t('healthy'), type: 'success' as const, title: '' }
  }

  if (healthyCount > 0) {
    return { label: t('partialHealthy'), type: 'warning' as const, title: errorTitle }
  }

  return { label: t('unhealthy'), type: 'danger' as const, title: errorTitle }
}

function channelRuntimeSummary(row: Channel) {
  if (!row.enabled) {
    return {
      status: 'disabled' as const,
      label: t('channelStopped'),
      icon: VideoPause,
      tone: 'neutral',
      title: ''
    }
  }

  const health = channelHealthSummary(row)
  if (health.type === 'success') {
    return {
      status: 'normal' as const,
      label: t('channelRunningNormal'),
      icon: CircleCheckFilled,
      tone: 'success',
      title: health.title
    }
  }

  return {
    status: 'attention' as const,
    label: health.type === 'warning' ? t('channelRunningDegraded') : t('channelServiceError'),
    icon: WarningFilled,
    tone: health.type === 'warning' ? 'warning' : 'danger',
    title: health.title
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
  try {
    const [fetchedPrices, fetchedTemplates] = await Promise.all([
      getProviderPrices(),
      getPricingTemplates()
    ])
    prices.value = fetchedPrices
    templates.value = fetchedTemplates
  } catch (err) {
    ElMessage.error(readError(err))
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
          ? template
            ? null
            : microUsdToUsd(inputPrice)
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
      ? null
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
  if (channelPriceStatus(channel).missing > 0) {
    openPriceDialog(channel)
  }
}

onMounted(loadPricingData)
</script>

<template>
  <section class="grid channel-management-view">
    <div class="channel-toolbar">
      <div class="channel-toolbar-filters">
        <el-input
          v-model="channelSearch"
          class="channel-search-input"
          clearable
          :placeholder="t('channelSearchPlaceholder')"
          :prefix-icon="Search"
        />
        <el-select v-model="channelStatusFilter" class="channel-status-filter">
          <el-option :label="t('allStatus')" value="all" />
          <el-option :label="t('channelRunningNormal')" value="normal" />
          <el-option :label="t('channelNeedsAttention')" value="attention" />
          <el-option :label="t('channelStopped')" value="disabled" />
        </el-select>
      </div>
      <el-button
        class="admin-action-button add-channel-action"
        type="primary"
        :icon="Plus"
        @click="openCreateDialog"
      >
        {{ t('addChannel') }}
      </el-button>
    </div>

    <el-table
      v-loading="loading"
      class="admin-table service-table channel-table"
      :data="filteredChannels"
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
              </div>
            </div>
          </div>
        </template>
      </el-table-column>
      <el-table-column prop="name" :label="t('name')" min-width="250">
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
      <el-table-column :label="t('modelPrices')" min-width="360">
        <template #default="{ row }">
          <div class="channel-price-summary">
            <div class="channel-price-summary-head">
              <el-tag
                v-if="channelPriceStatus(row).missing > 0"
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
      <el-table-column
        :label="t('channelStatus')"
        min-width="150"
        align="center"
        header-align="center"
      >
        <template #default="{ row }">
          <span class="channel-runtime-status" :class="`is-${channelRuntimeSummary(row).tone}`">
            <el-icon><component :is="channelRuntimeSummary(row).icon" /></el-icon>
            {{ channelRuntimeSummary(row).label }}
          </span>
        </template>
      </el-table-column>
      <el-table-column :label="t('actions')" width="164" align="center" header-align="center">
        <template #default="{ row }">
          <div class="table-row-actions">
            <el-button
              class="admin-action-button icon-only-action price-config-action"
              :aria-label="t('configurePrice')"
              :icon="Coin"
              @click="openPriceDialog(row)"
            />
            <el-button
              class="admin-action-button icon-only-action"
              :aria-label="t('edit')"
              :icon="Edit"
              @click="openEditDialog(row)"
            />
            <el-button
              :icon="Delete"
              class="admin-action-button icon-only-action"
              type="danger"
              :aria-label="t('delete')"
              :loading="deletingId === row.id"
              @click="confirmDeleteChannel(row)"
            />
          </div>
        </template>
      </el-table-column>
      <template #empty>
        <div class="channel-empty-state">
          <el-empty :description="channelSearch ? t('noMatchingChannels') : t('noChannels')">
            <el-button type="primary" :icon="Plus" @click="openCreateDialog">
              {{ t('addChannel') }}
            </el-button>
          </el-empty>
        </div>
      </template>
    </el-table>

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
      @fetch-models="fetchEditModels"
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
  </section>
</template>

<style scoped>
.channel-management-view {
  gap: 14px;
}

.channel-toolbar {
  align-items: center;
  display: flex;
  gap: 14px;
  justify-content: space-between;
  min-height: 42px;
  width: 100%;
}

.channel-toolbar-filters {
  align-items: center;
  display: flex;
  flex: 1 1 auto;
  flex-wrap: wrap;
  gap: 10px;
  min-width: 0;
}

.channel-search-input {
  width: min(340px, 100%);
}

.channel-status-filter {
  width: 150px;
}

.add-channel-action.el-button {
  box-shadow: 0 8px 18px rgba(22, 139, 211, 0.18);
  flex: 0 0 auto;
}

.add-channel-action.el-button:not(.is-disabled):hover {
  box-shadow: 0 10px 24px rgba(22, 139, 211, 0.24);
  transform: translateY(-1px);
}

.add-channel-action.el-button:not(.is-disabled):active {
  transform: translateY(0);
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

.channel-runtime-status {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #dbe4ef;
  border-radius: 8px;
  color: #4e5969;
  display: inline-flex;
  font-size: 12px;
  font-weight: 720;
  gap: 7px;
  min-height: 34px;
  padding: 0 13px 0 8px;
  white-space: nowrap;
}

.channel-runtime-status .el-icon {
  align-items: center;
  border-radius: 999px;
  color: #ffffff;
  display: inline-flex;
  font-size: 13px;
  height: 22px;
  justify-content: center;
  width: 22px;
}

.channel-runtime-status.is-success {
  background: #f0fdf4;
  border-color: #bbf7d0;
  color: #15803d;
}

.channel-runtime-status.is-success .el-icon {
  background: #22c55e;
}

.channel-runtime-status.is-warning {
  background: #fffbeb;
  border-color: #fde68a;
  color: #b45309;
}

.channel-runtime-status.is-warning .el-icon {
  background: #f59e0b;
}

.channel-runtime-status.is-danger {
  background: #fef2f2;
  border-color: #fecaca;
  color: #b91c1c;
}

.channel-runtime-status.is-danger .el-icon {
  background: #ef4444;
}

.channel-runtime-status.is-neutral {
  background: #eef2f6;
  border-color: #dbe4ef;
  color: #64748b;
}

.channel-runtime-status.is-neutral .el-icon {
  background: #94a3b8;
}

.channel-table :deep(.el-table__body td) {
  height: 82px;
  padding: 12px 0;
}

.channel-table :deep(.el-table__header-wrapper th) {
  background: #f6f9fc;
  color: #4e5969;
  font-size: 12px;
  font-weight: 760;
  height: 48px;
}

.channel-table :deep(.el-table__row:hover > td.el-table__cell) {
  background: #f7fbff;
}

.channel-table :deep(.el-table__expanded-cell) {
  background: #f6f9fc;
  padding: 0;
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

.channel-table :deep(.el-table__header-wrapper .cell) {
  overflow-wrap: normal;
  word-break: normal;
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

.price-status-tag.el-tag {
  border: 1px solid #dbe4ef;
  border-radius: 8px;
  color: #4e5969;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 720;
  gap: 6px;
  height: 34px;
  min-width: 0;
  padding: 0 13px 0 8px;
}

.price-status-tag.el-tag .el-icon {
  align-items: center;
  background: #94a3b8;
  border-radius: 999px;
  color: #ffffff;
  display: inline-flex;
  font-size: 13px;
  height: 22px;
  justify-content: center;
  width: 22px;
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
  min-width: 0;
  overflow: hidden;
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
  min-width: 94px;
  padding: 2px 9px;
  text-align: right;
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

.channel-expand-panel {
  animation: channel-expand-in 180ms ease-out;
  display: grid;
  gap: 14px;
  margin: 6px 12px 14px 44px;
  padding: 16px;
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
  box-shadow: 0 6px 14px rgba(22, 139, 211, 0.16);
}

.expand-price-action.el-button:not(.is-disabled):hover {
  box-shadow: 0 10px 24px rgba(22, 139, 211, 0.22);
  transform: translateY(-1px);
}

.expand-price-action.el-button:not(.is-disabled):active {
  transform: translateY(0);
}

.channel-expand-price-table {
  background: #ffffff;
  border: 1px solid #e3ebf4;
  border-radius: 8px;
  overflow: hidden;
}

.channel-expand-price-row {
  align-items: center;
  background: #ffffff;
  display: grid;
  gap: 10px;
  grid-template-columns:
    minmax(160px, 1fr)
    minmax(112px, 0.42fr)
    minmax(112px, 0.42fr)
    minmax(112px, 0.42fr)
    minmax(112px, 0.42fr)
    104px;
  min-height: 46px;
  padding: 0 14px;
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
  padding-left: 40px;
  text-align: center;
}

.channel-expand-price-row.is-missing {
  background: #fffaf3;
}

.channel-expand-price-row.is-missing .channel-price-model {
  color: #c2410c;
}

.channel-expand-price-row .channel-price-model {
  background: transparent;
  border: 0;
  border-radius: 0;
  color: #1d2129;
  display: inline-block;
  font-size: 13px;
  font-weight: 680;
  max-width: 100%;
  padding: 0;
}

.channel-detail-price {
  align-items: center;
  color: #1d2129;
  display: inline-flex;
  font-size: 13px;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 720;
  justify-content: flex-end;
  white-space: nowrap;
}

.channel-detail-status {
  align-items: center;
  display: inline-flex;
  justify-content: center;
  padding-left: 40px;
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

.channel-expand-price-row.is-missing .channel-detail-price,
.channel-expand-price-row.is-missing .channel-detail-status {
  color: #c2410c;
}

.channel-expand-price-row.is-disabled {
  opacity: 0.56;
}

.table-row-actions {
  display: flex;
  gap: 8px;
  justify-content: center;
  white-space: nowrap;
}

.table-row-actions .el-button {
  margin-left: 0;
}

.table-row-actions .icon-only-action {
  min-width: 36px;
  padding-left: 0;
  padding-right: 0;
  width: 36px;
}

.table-row-actions :deep(.el-button:not(.el-button--danger)) {
  --el-button-bg-color: #fbfdff;
  --el-button-border-color: #d5e0ec;
  --el-button-hover-bg-color: var(--brand-blue-soft);
  --el-button-hover-border-color: var(--brand-blue-border);
  --el-button-hover-text-color: var(--brand-blue);
}

.table-row-actions :deep(.price-config-action) {
  --el-button-bg-color: var(--brand-blue-soft);
  --el-button-border-color: var(--brand-blue-border);
  --el-button-text-color: var(--brand-blue);
}

.table-row-actions :deep(.el-button--danger) {
  --el-button-bg-color: #fff1f2;
  --el-button-border-color: #fecdd3;
  --el-button-text-color: #dc2626;
  --el-button-hover-bg-color: #fee2e2;
  --el-button-hover-border-color: #fda4af;
  --el-button-hover-text-color: #be123c;
}

.table-row-actions :deep(.el-button--danger:not(.is-disabled):hover) {
  --el-button-bg-color: #ef4444;
  --el-button-border-color: #ef4444;
  --el-button-hover-bg-color: #ef4444;
  --el-button-hover-border-color: #ef4444;
  --el-button-hover-text-color: #ffffff;
  color: #ffffff;
  transform: translateY(-1px) scale(1.04);
}

.table-row-actions :deep(.el-button:not(.is-disabled):hover) {
  box-shadow: 0 6px 14px rgba(15, 23, 42, 0.08);
  color: var(--brand-blue);
  transform: translateY(-1px);
}

.table-row-actions :deep(.el-button:not(.is-disabled):active) {
  transform: translateY(0);
}

.channel-empty-state {
  padding: 30px 0 34px;
}

@keyframes channel-expand-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }

  to {
    opacity: 1;
    transform: translateY(0);
  }
}

@media (max-width: 760px) {
  .channel-toolbar {
    align-items: stretch;
    display: grid;
    grid-template-columns: 1fr;
  }

  .channel-toolbar-filters {
    display: grid;
    grid-template-columns: 1fr;
  }

  .channel-search-input,
  .channel-status-filter {
    width: 100%;
  }

  .add-channel-action.el-button {
    width: 100%;
  }

  .channel-expand-panel {
    margin-left: 0;
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
