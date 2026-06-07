<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { Coin, Delete, Edit } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getPricingTemplates, getProviderPrices, upsertProviderPrice } from '../../api/prices'
import ChannelFormDialog from '../../components/admin/channels/ChannelFormDialog.vue'
import ChannelPriceDialog, {
  type ChannelPriceForm
} from '../../components/admin/channels/ChannelPriceDialog.vue'
import ModelPickerDialog from '../../components/admin/channels/ModelPickerDialog.vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useChannels } from '../../composables/useChannels'
import { useLocale } from '../../composables/useLocale'
import type { Channel, PricingTemplate, ProviderPrice } from '../../types/admin'
import { readError } from '../../utils/errors'
import { formatUsdPerMillion, microUsdToUsd, usdToMicroUsd } from '../../utils/format'
import { findPricingTemplate, priceKey } from '../../utils/pricing'

const { t } = useLocale()

const {
  channels,
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
const priceForms = reactive<Record<string, ChannelPriceForm>>({})

const priceByModel = computed(
  () => new Map(prices.value.map((price) => [priceKey(price.provider, price.model), price]))
)
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
  return {
    missing,
    total: models.length,
    type: 'warning' as const,
    label: `${t('priceMissing')} ${missing}/${models.length}`
  }
}

function channelPriceRows(row: Channel) {
  return channelModelList(row).map((model) => {
    const price = priceByModel.value.get(priceKey(row.provider, model))
    return {
      model,
      disabled: Boolean(price && !price.enabled),
      missing: !price,
      price: price
        ? `${formatUsdPerMillion(microUsdToUsd(price.input_price_usd_micros))} / ${formatUsdPerMillion(microUsdToUsd(price.output_price_usd_micros))}`
        : t('priceMissing')
    }
  })
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
  const channel = await submitChannelBase()
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
  <section class="grid">
    <div class="table-toolbar">
      <el-button class="admin-action-button" type="primary" @click="openCreateDialog">
        {{ t('addChannel') }}
      </el-button>
    </div>

    <el-table
      v-loading="loading"
      class="admin-table service-table channel-table"
      :data="channels"
      stripe
    >
      <el-table-column prop="name" :label="t('name')" width="260">
        <template #default="{ row }">
          <span class="channel-name-cell">
            <ProviderIcon :provider="row.provider" />
            <span class="channel-name-text">{{ row.name }}</span>
          </span>
        </template>
      </el-table-column>
      <el-table-column :label="t('modelPrices')" min-width="270">
        <template #default="{ row }">
          <div class="channel-price-list">
            <span
              v-for="item in channelPriceRows(row)"
              :key="item.model"
              class="channel-price-item"
              :class="{ 'is-missing': item.missing, 'is-disabled': item.disabled }"
              :title="`${item.model}: ${item.price}`"
            >
              <span class="channel-price-model">{{ item.model }}</span>
              <span class="channel-price-value">{{ item.price }}</span>
            </span>
          </div>
        </template>
      </el-table-column>
      <el-table-column :label="t('channelKeyCountShort')" width="90" align="center">
        <template #default="{ row }">
          <span class="channel-key-count">{{
            row.use_credentials ? t('credentialFiles') : (keyCounts.get(row.id) ?? 0)
          }}</span>
        </template>
      </el-table-column>
      <el-table-column :label="t('channelStatus')" width="100" align="center">
        <template #default="{ row }">
          <el-tag class="static-state-tag" :type="row.enabled ? 'success' : 'info'">
            {{ row.enabled ? t('enabled') : t('disabled') }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column :label="t('health')" width="100" align="center">
        <template #default="{ row }">
          <el-tag
            class="static-state-tag health-summary-tag"
            :title="channelHealthSummary(row).title"
            :type="channelHealthSummary(row).type"
          >
            {{ channelHealthSummary(row).label }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column :label="t('actions')" width="260" align="center" header-align="center">
        <template #default="{ row }">
          <div class="table-row-actions">
            <el-button
              class="admin-action-button price-config-action"
              :icon="Coin"
              @click="openPriceDialog(row)"
            >
              {{ t('configurePrice') }}
            </el-button>
            <el-button
              class="admin-action-button icon-only-action"
              :aria-label="t('edit')"
              :icon="Edit"
              :title="t('edit')"
              @click="openEditDialog(row)"
            >
              {{ t('edit') }}
            </el-button>
            <el-button
              :icon="Delete"
              class="admin-action-button icon-only-action"
              type="danger"
              :aria-label="t('delete')"
              :loading="deletingId === row.id"
              :title="t('delete')"
              @click="confirmDeleteChannel(row)"
            >
              {{ t('delete') }}
            </el-button>
          </div>
        </template>
      </el-table-column>
      <template #empty>
        <el-empty :description="t('noData')" />
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
.channel-name-cell {
  align-items: center;
  display: inline-flex;
  gap: 10px;
  max-width: 100%;
  min-width: 0;
  vertical-align: middle;
}

.channel-name-text {
  color: #111827;
  font-size: 14px;
  font-weight: 720;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.static-state-tag,
.static-state-tag * {
  animation: none !important;
  transition: none !important;
}

.health-summary-tag {
  min-width: 72px;
  justify-content: center;
}

.channel-table :deep(.el-table__body td) {
  height: 72px;
}

.channel-table :deep(.el-table__header-wrapper .cell) {
  overflow-wrap: normal;
  word-break: normal;
}

.channel-table :deep(.el-table__body .cell) {
  align-items: center;
  display: flex;
}

.channel-table :deep(.el-table__body .el-table__cell:nth-child(2) .cell) {
  display: block;
}

.channel-table :deep(.el-table__body .el-table__cell:last-child .cell) {
  justify-content: flex-end;
}

.channel-price-list {
  display: grid;
  gap: 7px;
  justify-items: start;
  min-width: 0;
  max-width: 100%;
  width: fit-content;
}

.channel-price-item {
  align-items: center;
  display: inline-flex;
  gap: 10px;
  min-width: 0;
  padding: 1px 0;
}

.channel-price-model {
  color: #334155;
  font-size: 13px;
  font-weight: 720;
  letter-spacing: 0;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-price-value {
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 999px;
  color: #0f172a;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-weight: 800;
  min-width: 108px;
  padding: 2px 9px;
  text-align: center;
  white-space: nowrap;
}

.channel-price-item.is-missing .channel-price-value {
  background: #fff8eb;
  border-color: #fde4b3;
  color: #d97706;
}

.channel-price-item.is-disabled {
  opacity: 0.56;
}

.channel-key-count {
  color: #1f2937;
  display: inline-flex;
  font-size: 14px;
  font-variant-numeric: tabular-nums;
  font-weight: 760;
  justify-content: center;
  min-width: 22px;
}

.table-toolbar {
  display: flex;
  justify-content: flex-end;
}

.table-toolbar :deep(.el-button),
.table-row-actions :deep(.el-button) {
  border-radius: 6px;
}

.table-row-actions {
  display: flex;
  gap: 6px;
  justify-content: center;
  white-space: nowrap;
}

.table-row-actions .el-button {
  margin-left: 0;
  padding-left: 10px;
  padding-right: 10px;
}

.table-row-actions .price-config-action {
  min-width: 94px;
}

.table-row-actions .icon-only-action {
  min-width: 58px;
  padding-left: 10px;
  padding-right: 10px;
  width: auto;
}

.table-row-actions :deep(.el-button:not(.el-button--danger)) {
  --el-button-bg-color: #ffffff;
  --el-button-border-color: #d8dee8;
  --el-button-hover-bg-color: #f8fbff;
  --el-button-hover-border-color: #b8c7dc;
  --el-button-hover-text-color: var(--brand-blue);
}

.table-row-actions :deep(.el-button--danger) {
  --el-button-bg-color: #ff6b6b;
  --el-button-border-color: #ff6b6b;
  --el-button-hover-bg-color: #f04438;
  --el-button-hover-border-color: #f04438;
}
</style>
