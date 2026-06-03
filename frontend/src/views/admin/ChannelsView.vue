<script setup lang="ts">
import { computed, h, onMounted, reactive, ref } from 'vue'
import { Coin, Delete, Edit } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  getPricingTemplates,
  getProviderPrices,
  syncPricingTemplates,
  upsertProviderPrice
} from '../../api/prices'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useChannels } from '../../composables/useChannels'
import { useLocale } from '../../composables/useLocale'
import type { Channel, PricingTemplate, ProviderPrice } from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { findPricingTemplate, priceKey } from '../../utils/pricing'

const { locale, t } = useLocale()

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
const syncingTemplates = ref(false)
const priceForms = reactive<Record<string, {
  provider: string
  model: string
  inputUsdPerMillion: number
  outputUsdPerMillion: number
  cacheReadUsdPerMillion: number
  cacheWriteUsdPerMillion: number | null
  enabled: boolean
  hasPrice: boolean
  templateSource?: string
}>>({})

const MICRO_USD_PER_USD = 1_000_000

const priceByModel = computed(() => new Map(prices.value.map((price) => [priceKey(price.provider, price.model), price])))
function channelModelList(row: Channel) {
  const models = row.endpoints.flatMap((endpoint) => endpoint.models)
  return Array.from(new Set(models.map((model) => model.trim()).filter(Boolean)))
}

function microUsdToUsd(value: number) {
  return value / MICRO_USD_PER_USD
}

function usdToMicroUsd(value: number) {
  return Math.round(value * MICRO_USD_PER_USD)
}

function formatUsdPerMillion(value: number) {
  return `$${value.toLocaleString('en-US', {
    maximumFractionDigits: 6
  })}`
}

function derivedCacheReadPrice(inputPrice: number) {
  return Math.round(inputPrice / 10)
}

function formatSyncCount(value: number) {
  return value.toLocaleString('en-US')
}

function referencePricesSyncedMessage(result: { saved: number; fetched: number; skipped: number }) {
  if (locale.value === 'zh-CN') {
    return `${t('referencePricesSynced')}：已保存 ${formatSyncCount(result.saved)} 条，源数据 ${formatSyncCount(result.fetched)} 个模型，跳过 ${formatSyncCount(result.skipped)} 条`
  }

  return `${t('referencePricesSynced')}: saved ${formatSyncCount(result.saved)}, source models ${formatSyncCount(result.fetched)}, skipped ${formatSyncCount(result.skipped)}.`
}

function referenceSyncConfirmContent() {
  return h('div', { class: 'reference-sync-copy' }, [
    h('p', { class: 'reference-sync-lead' }, t('syncReferencePricesConfirmIntro')),
    h('div', { class: 'reference-sync-notes' }, [
      h('p', t('syncReferencePricesConfirmSafe')),
      h('p', t('syncReferencePricesConfirmApply'))
    ])
  ])
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

function channelPriceStatus(row: Channel) {
  const models = channelModelList(row)
  if (models.length === 0) {
    return { missing: 0, total: 0, type: 'info' as const, label: '-' }
  }

  const missing = models.filter((model) => !priceByModel.value.get(priceKey(row.provider, model))?.enabled).length
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

async function syncReferencePrices() {
  try {
    await ElMessageBox.confirm(referenceSyncConfirmContent(), t('syncReferencePricesConfirmTitle'), {
      confirmButtonText: t('syncReferencePricesConfirmButton'),
      cancelButtonText: t('cancel'),
      customClass: 'reference-sync-confirm'
    })
  } catch {
    return
  }

  syncingTemplates.value = true
  try {
    const result = await syncPricingTemplates()
    ElMessage.success(referencePricesSyncedMessage(result))
    await loadPricingData()
  } catch (err) {
    ElMessage.error(readReferenceSyncError(err))
  } finally {
    syncingTemplates.value = false
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
      : price?.cache_write_price_usd_micros ?? inputPrice
    priceForms[key] = {
      provider: row.provider,
      model,
      inputUsdPerMillion: microUsdToUsd(inputPrice),
      outputUsdPerMillion: microUsdToUsd(price?.output_price_usd_micros ?? template?.output_price_usd_micros ?? 0),
      cacheReadUsdPerMillion: microUsdToUsd(
        price?.cache_read_price_usd_micros ?? template?.cache_read_price_usd_micros ?? derivedCacheReadPrice(inputPrice)
      ),
      cacheWriteUsdPerMillion: cacheWritePrice === undefined || cacheWritePrice === null
        ? template ? null : microUsdToUsd(inputPrice)
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
  const cacheRead = formatUsdPerMillion(microUsdToUsd(template.cache_read_price_usd_micros ?? derivedCacheReadPrice(template.input_price_usd_micros)))
  const cacheWrite = template.cache_write_price_usd_micros === undefined || template.cache_write_price_usd_micros === null
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
  form.cacheWriteUsdPerMillion = template.cache_write_price_usd_micros === undefined || template.cache_write_price_usd_micros === null
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
      <el-button class="admin-action-button" :loading="syncingTemplates" @click="syncReferencePrices">
        {{ t('syncReferencePrices') }}
      </el-button>
      <el-button class="admin-action-button" type="primary" @click="openCreateDialog">
        {{ t('addChannel') }}
      </el-button>
    </div>

    <el-table v-loading="loading" class="admin-table service-table channel-table" :data="channels" stripe>
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
          <span class="channel-key-count">{{ row.use_credentials ? t('credentialFiles') : (keyCounts.get(row.id) ?? 0) }}</span>
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
            <el-button class="admin-action-button price-config-action" :icon="Coin" @click="openPriceDialog(row)">
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
    </el-table>

    <el-dialog v-model="createDialogOpen" class="channel-dialog" :title="t('createChannel')" width="620px">
      <el-form class="channel-form" label-position="top" @submit.prevent="submitChannel">
        <div class="provider-row">
          <el-form-item class="provider-field" :label="t('provider')">
            <el-select
              v-model="createForm.provider"
              class="provider-select"
              filterable
              popper-class="provider-select-dropdown"
              @change="selectCreateProvider"
            >
              <template #prefix>
                <ProviderIcon :provider="createForm.provider" />
              </template>
              <el-option
                v-for="provider in providerOptions"
                :key="provider.value"
                :label="provider.label"
                :value="provider.value"
              >
                <span class="provider-option">
                  <ProviderIcon :provider="provider.value" />
                  <span class="provider-option-label">{{ provider.label }}</span>
                </span>
              </el-option>
            </el-select>
          </el-form-item>

          <label class="status-toggle">
            <span>{{ t('status') }}</span>
            <el-switch v-model="createForm.enabled" />
          </label>
        </div>

        <el-form-item :label="t('name')">
          <el-input v-model="createForm.name" :placeholder="t('channelNamePlaceholder')" />
        </el-form-item>

        <el-form-item :label="t('baseUrl')">
          <el-input
            v-model="createBaseUrl"
            class="base-url-input"
            :class="{ 'is-readonly': isCreateBaseUrlReadonly }"
            :placeholder="t('baseUrlPlaceholder')"
            :readonly="isCreateBaseUrlReadonly"
          />
        </el-form-item>

        <el-form-item :label="t('models')">
          <div class="models-row">
            <el-input
              v-model="createForm.models"
              :placeholder="modelsInputPlaceholder()"
              :readonly="modelsInputReadonly()"
            />
            <button
              class="auto-fetch-link"
              :class="{ 'is-loading': fetchingModels }"
              type="button"
              :disabled="fetchingModels"
              @click="fetchCreateModels"
            >
              {{ fetchingModels ? t('fetchingModels') : t('autoFetch') }}
            </button>
          </div>
        </el-form-item>

        <div class="dialog-section-title">
          <span>{{ createForm.use_credentials ? t('credentialFiles') : t('upstreamApiKey') }}</span>
        </div>

        <label class="credential-source-toggle">
          <span>{{ t('useCredentialFiles') }}</span>
          <el-switch v-model="createForm.use_credentials" />
        </label>

        <el-form-item v-if="!createForm.use_credentials" class="api-key-field" :label="t('apiKeyOrJson')">
          <el-input
            v-model="secretInput"
            class="secret-input"
            :rows="5"
            type="textarea"
            :placeholder="t('optionalUpstreamKey')"
          />
        </el-form-item>

        <button class="hidden-submit" type="submit" />
      </el-form>

      <template #footer>
        <div class="dialog-footer">
          <el-button @click="createDialogOpen = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="creating" @click="submitChannel">
            {{ t('create') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="modelPickerDialogOpen"
      class="model-picker-dialog"
      :title="t('selectModels')"
      width="560px"
      append-to-body
    >
      <div class="model-picker">
        <div class="model-picker-toolbar">
          <span class="model-count">
            {{ t('selectedModelCount') }} {{ selectedFetchedModels.length }}/{{ fetchedModels.length }}
          </span>
        </div>

        <div class="model-select-panel">
          <div class="model-checkbox-list">
            <label class="model-checkbox-item model-checkbox-all">
              <input
                type="checkbox"
                :checked="allFetchedModelsSelected"
                @change="toggleAllFetchedModels(($event.target as HTMLInputElement).checked)"
              />
              <span>{{ t('allModels') }}</span>
            </label>
            <label
              v-for="model in fetchedModels"
              :key="model"
              class="model-checkbox-item"
            >
              <input v-model="selectedFetchedModels" type="checkbox" :value="model" />
              <span>{{ model }}</span>
            </label>
          </div>
        </div>
      </div>

      <template #footer>
        <div class="dialog-footer">
          <el-button type="primary" @click="modelPickerDialogOpen = false">
            {{ t('save') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog v-model="editDialogOpen" class="channel-dialog" :title="t('editChannel')" width="620px">
      <el-form class="channel-form" label-position="top" @submit.prevent="submitEditChannel">
        <div class="provider-row">
          <el-form-item class="provider-field" :label="t('provider')">
            <el-select
              :model-value="editingChannel?.provider"
              class="provider-select"
              disabled
              popper-class="provider-select-dropdown"
            >
              <template #prefix>
                <ProviderIcon :provider="editingChannel?.provider ?? ''" />
              </template>
              <el-option
                v-for="provider in providerOptions"
                :key="provider.value"
                :label="provider.label"
                :value="provider.value"
              >
                <span class="provider-option">
                  <ProviderIcon :provider="provider.value" />
                  <span class="provider-option-label">{{ provider.label }}</span>
                </span>
              </el-option>
            </el-select>
          </el-form-item>

          <label class="status-toggle">
            <span>{{ t('status') }}</span>
            <el-switch v-model="editForm.enabled" />
          </label>
        </div>

        <el-form-item :label="t('name')">
          <el-input v-model="editForm.name" :placeholder="t('channelNamePlaceholder')" />
        </el-form-item>

        <el-form-item :label="t('baseUrl')">
          <el-input
            v-model="editBaseUrl"
            class="base-url-input"
            :class="{ 'is-readonly': isEditBaseUrlReadonly }"
            :placeholder="t('baseUrlPlaceholder')"
            :readonly="isEditBaseUrlReadonly"
          />
        </el-form-item>

        <el-form-item :label="t('models')">
          <div class="models-row">
            <el-input
              v-model="editForm.models"
              :placeholder="modelsInputPlaceholder()"
              :readonly="modelsInputReadonly()"
            />
            <button
              class="auto-fetch-link"
              :class="{ 'is-loading': fetchingModels }"
              type="button"
              :disabled="fetchingModels"
              @click="fetchEditModels"
            >
              {{ fetchingModels ? t('fetchingModels') : t('autoFetch') }}
            </button>
          </div>
        </el-form-item>

        <div class="dialog-section-title">
          <span>{{ editForm.use_credentials ? t('credentialFiles') : t('upstreamApiKey') }}</span>
        </div>

        <label class="credential-source-toggle">
          <span>{{ t('useCredentialFiles') }}</span>
          <el-switch v-model="editForm.use_credentials" />
        </label>

        <el-form-item v-if="!editForm.use_credentials" class="api-key-field" :label="t('apiKeyOrJson')">
          <el-input
            v-model="editSecretInput"
            class="secret-input"
            :rows="5"
            type="textarea"
            :placeholder="t('optionalEditUpstreamKey')"
          />
        </el-form-item>

        <button class="hidden-submit" type="submit" />
      </el-form>

      <template #footer>
        <div class="dialog-footer">
          <el-button @click="editDialogOpen = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="updating" @click="submitEditChannel">
            {{ t('save') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="priceDialogOpen"
      class="channel-dialog price-dialog"
      :title="t('configurePrice')"
      width="min(720px, calc(100vw - 32px))"
    >
      <div class="price-editor">
        <div class="price-editor-head">
          <span>{{ t('model') }}</span>
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
          <div v-for="row in Object.values(priceForms)" :key="`${row.provider}:${row.model}`" class="price-editor-row">
            <div class="price-model-cell" :title="row.model">
              <ProviderIcon :provider="priceIconProvider(row)" class="price-model-icon" />
              <span>{{ row.model }}</span>
            </div>
            <div class="price-pair-field">
              <div class="price-pair-input">
                <el-input-number
                  v-model="row.inputUsdPerMillion"
                  class="price-number-input"
                  :controls="false"
                  :min="0"
                  :step="0.01"
                />
                <span class="price-pair-separator">/</span>
                <el-input-number
                  v-model="row.outputUsdPerMillion"
                  class="price-number-input"
                  :controls="false"
                  :min="0"
                  :step="0.01"
                />
              </div>
            </div>
            <div class="price-pair-field">
              <div class="price-pair-input">
                <el-input-number
                  v-model="row.cacheReadUsdPerMillion"
                  class="price-number-input"
                  :controls="false"
                  :min="0"
                  :step="0.01"
                />
                <span class="price-pair-separator">/</span>
                <el-input-number
                  v-if="row.cacheWriteUsdPerMillion !== null"
                  v-model="row.cacheWriteUsdPerMillion"
                  class="price-number-input"
                  :controls="false"
                  :min="0"
                  :step="0.01"
                />
                <el-input-number
                  v-else
                  class="price-number-input is-cache-write-free"
                  :model-value="0"
                  :controls="false"
                  :min="0"
                  :step="0.01"
                  disabled
                />
              </div>
            </div>
            <div class="reference-price-cell">
              <template v-if="hasReferencePrice(row)">
                <span class="reference-price-summary">{{ referencePriceSummary(row) }}</span>
                <span class="reference-price-source">{{ row.templateSource || t('pricingTemplate') }}</span>
              </template>
              <el-tag v-else :type="row.hasPrice ? 'info' : 'warning'">
                {{ referencePriceFallbackLabel(row) }}
              </el-tag>
            </div>
          </div>
        </div>
      </div>

      <template #footer>
        <div class="dialog-footer price-dialog-footer">
          <el-button :loading="savingPrices" @click="applyReferencePrices">
            {{ t('applyReferencePrices') }}
          </el-button>
          <div class="price-dialog-actions">
            <el-button @click="priceDialogOpen = false">{{ t('cancel') }}</el-button>
            <el-button type="primary" :loading="savingPrices" @click="saveChannelPrices">
              {{ t('save') }}
            </el-button>
          </div>
        </div>
      </template>
    </el-dialog>

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

.single-line {
  display: block;
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

.credential-source-toggle {
  align-items: center;
  color: #334155;
  display: flex;
  font-size: 13px;
  font-weight: 700;
  justify-content: space-between;
}

.table-toolbar {
  display: flex;
  justify-content: flex-end;
}

.table-toolbar :deep(.el-button),
.table-row-actions :deep(.el-button) {
  border-radius: 6px;
}

.channel-form {
  display: grid;
  gap: 13px;
}

.provider-row {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr) 86px;
}

.provider-field {
  margin-bottom: 0;
}

.provider-select {
  width: 100%;
}

.provider-select :deep(.el-select__prefix) {
  left: 12px;
}

.provider-select :deep(.el-select__wrapper) {
  gap: 5px;
}

.provider-select :deep(.el-select__placeholder) {
  padding-left: 2px;
}

.provider-option {
  align-items: center;
  display: flex;
  gap: 5px;
  min-width: 0;
}

.provider-option-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

:global(.provider-select-dropdown .el-select-dropdown__item) {
  height: 42px;
  line-height: 42px;
  padding: 0 14px;
}

:global(.provider-select-dropdown .el-select-dropdown__item.selected .provider-icon) {
  border-color: currentColor;
}

.status-toggle {
  align-items: center;
  align-self: end;
  color: #475569;
  display: flex;
  font-size: 14px;
  font-weight: 720;
  gap: 8px;
  justify-content: flex-end;
  min-height: 42px;
}

.base-url-input.is-readonly :deep(.el-input__wrapper) {
  background: #f8fafc;
  box-shadow: 0 0 0 1px #e3e8ef inset;
}

.base-url-input.is-readonly :deep(.el-input__inner) {
  color: #667085;
  -webkit-text-fill-color: #667085;
}

.dialog-section-title {
  align-items: center;
  color: #334155;
  display: grid;
  font-size: 14px;
  font-weight: 760;
  gap: 14px;
  grid-template-columns: 1fr auto 1fr;
  margin: 6px 0 -1px;
}

.dialog-section-title::before,
.dialog-section-title::after {
  background: #dfe4ec;
  content: "";
  height: 1px;
}

.api-key-field {
  margin-bottom: 0;
}

.dialog-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

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
    minmax(140px, 0.7fr)
    minmax(170px, 0.8fr)
    minmax(170px, 0.8fr)
    minmax(170px, 0.84fr);
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
  padding: 0 10px;
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

.price-pair-input {
  align-items: center;
  background: #ffffff;
  border: 1px solid #d8e0ec;
  border-radius: 6px;
  display: flex;
  gap: 3px;
  min-height: 34px;
  padding: 0 6px;
  width: 140px;
}

.price-number-input {
  flex: 0 1 58px;
  min-width: 0;
  width: 58px;
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
  text-align: center;
}

.price-number-input.is-cache-write-free :deep(.el-input__wrapper) {
  background: transparent;
  cursor: default;
}

.price-number-input.is-cache-write-free :deep(.el-input__inner) {
  color: #94a3b8;
  -webkit-text-fill-color: #94a3b8;
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

.price-dialog-footer {
  align-items: center;
  justify-content: space-between;
}

.price-dialog-actions {
  display: flex;
  gap: 12px;
}

.models-row {
  align-items: center;
  display: grid;
  gap: 10px;
  grid-template-columns: minmax(0, 1fr) auto;
  width: 100%;
}

.auto-fetch-link {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  color: var(--brand-blue);
  cursor: pointer;
  display: inline-flex;
  font: inherit;
  font-size: 14px;
  font-weight: 740;
  gap: 6px;
  min-height: 42px;
  padding: 0 2px;
  text-decoration: underline;
  text-underline-offset: 3px;
  white-space: nowrap;
}

.auto-fetch-link:disabled {
  color: #98a2b3;
  cursor: default;
}

.auto-fetch-link.is-loading::before {
  animation: fetch-spin 0.8s linear infinite;
  border: 2px solid #c7d7fe;
  border-top-color: var(--brand-blue);
  border-radius: 999px;
  content: "";
  height: 13px;
  width: 13px;
}

@keyframes fetch-spin {
  to {
    transform: rotate(360deg);
  }
}

.model-picker {
  display: grid;
  gap: 10px;
}

.model-picker-toolbar {
  align-items: center;
  display: flex;
  justify-content: flex-start;
}

.model-count {
  color: #667085;
  font-size: 13px;
  font-weight: 700;
  white-space: nowrap;
}

.model-select-panel {
  display: grid;
  gap: 8px;
}

.model-checkbox-list {
  align-content: start;
  background: #ffffff;
  border: 1px solid #e3e8ef;
  border-radius: 8px;
  display: grid;
  gap: 0;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  max-height: 270px;
  overflow: auto;
  padding: 6px;
}

.model-checkbox-item {
  align-items: center;
  border-radius: 6px;
  color: #475569;
  cursor: pointer;
  display: grid;
  gap: 9px;
  grid-template-columns: 16px minmax(0, 1fr);
  height: 32px;
  min-width: 0;
  padding: 0 7px;
}

.model-checkbox-item:hover {
  background: #f8fafc;
}

.model-checkbox-all {
  border-bottom: 1px solid #edf1f6;
  color: #1f2937;
  font-weight: 760;
  grid-column: 1 / -1;
  margin-bottom: 3px;
}

.model-checkbox-item input {
  accent-color: var(--brand-blue);
  cursor: pointer;
  height: 16px;
  margin: 0;
  width: 16px;
}

.model-checkbox-item span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

:global(.channel-dialog),
:global(.model-picker-dialog) {
  border-radius: 10px;
  max-width: calc(100vw - 32px);
}

:global(.channel-dialog .el-dialog__header),
:global(.model-picker-dialog .el-dialog__header) {
  margin: 0;
}

:global(.channel-dialog .el-dialog__header) {
  padding: 28px 32px 8px;
}

:global(.model-picker-dialog .el-dialog__header) {
  padding: 22px 24px 10px;
}

:global(.channel-dialog .el-dialog__title),
:global(.model-picker-dialog .el-dialog__title) {
  color: #111827;
  line-height: 1.2;
}

:global(.channel-dialog .el-dialog__title) {
  font-size: 25px;
  font-weight: 820;
}

:global(.model-picker-dialog .el-dialog__title) {
  font-size: 21px;
  font-weight: 800;
}

:global(.channel-dialog .el-dialog__headerbtn) {
  right: 22px;
  top: 20px;
}

:global(.model-picker-dialog .el-dialog__headerbtn) {
  right: 18px;
  top: 14px;
}

:global(.channel-dialog .el-dialog__body) {
  padding: 14px 32px 18px;
}

:global(.model-picker-dialog .el-dialog__body) {
  padding: 14px 24px 8px;
}

:global(.channel-dialog .el-dialog__footer),
:global(.model-picker-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
}

:global(.channel-dialog .el-dialog__footer) {
  padding: 18px 32px 24px;
}

:global(.model-picker-dialog .el-dialog__footer) {
  padding: 14px 24px 20px;
}

:global(.price-dialog .el-dialog__header) {
  padding: 15px 18px 5px;
}

:global(.price-dialog .el-dialog__title) {
  font-size: 18px;
  font-weight: 620;
}

:global(.price-dialog .el-dialog__headerbtn) {
  right: 12px;
  top: 10px;
}

:global(.price-dialog .el-dialog__body) {
  padding: 8px 18px 10px;
}

:global(.price-dialog .el-dialog__footer) {
  padding: 10px 18px 14px;
}

:global(.price-dialog .dialog-footer .el-button) {
  border-radius: 6px;
  font-weight: 600;
  min-height: 32px;
  min-width: 70px;
}

:global(.reference-sync-confirm) {
  border-radius: 12px;
  box-shadow: 0 22px 60px rgba(15, 23, 42, 0.18);
  max-width: calc(100vw - 32px);
  padding: 0;
  width: 520px;
}

:global(.reference-sync-confirm .el-message-box__header) {
  padding: 22px 24px 6px;
}

:global(.reference-sync-confirm .el-message-box__title) {
  color: #111827;
  font-size: 22px;
  font-weight: 820;
  line-height: 1.25;
}

:global(.reference-sync-confirm .el-message-box__headerbtn) {
  right: 18px;
  top: 16px;
}

:global(.reference-sync-confirm .el-message-box__content) {
  padding: 10px 24px 18px;
}

:global(.reference-sync-confirm .el-message-box__container) {
  display: block;
}

:global(.reference-sync-confirm .el-message-box__status) {
  display: none;
}

:global(.reference-sync-confirm .el-message-box__message) {
  color: #4b5563;
  font-size: 14px;
  font-weight: 400;
  line-height: 1.6;
  margin: 0;
  padding: 0;
}

:global(.reference-sync-confirm .el-message-box__message p) {
  margin: 0;
}

:global(.reference-sync-confirm .reference-sync-copy) {
  display: grid;
  gap: 12px;
}

:global(.reference-sync-confirm .reference-sync-lead) {
  color: #374151;
  font-size: 15px;
  line-height: 1.65;
}

:global(.reference-sync-confirm .reference-sync-notes) {
  background: #f8fafc;
  border: 1px solid #e5eaf1;
  border-radius: 8px;
  display: grid;
  gap: 8px;
  padding: 12px 14px;
}

:global(.reference-sync-confirm .reference-sync-notes p) {
  color: #64748b;
  line-height: 1.55;
}

:global(.reference-sync-confirm .el-message-box__btns) {
  gap: 10px;
  padding: 0 24px 24px;
}

:global(.reference-sync-confirm .el-message-box__btns .el-button) {
  border-radius: 8px;
  font-size: 15px;
  font-weight: 760;
  min-height: 38px;
  min-width: 86px;
}

.channel-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.channel-form :deep(.el-form-item__label) {
  color: #475569;
  font-size: 14px;
  font-weight: 740;
  line-height: 1.25;
  margin-bottom: 7px;
}

.channel-form :deep(.el-input__wrapper),
.channel-form :deep(.el-select__wrapper) {
  border-radius: 8px;
  min-height: 42px;
}

.channel-form :deep(.el-input__inner) {
  font-size: 15px;
}

.channel-form :deep(.el-textarea__inner) {
  border-radius: 8px;
  font-size: 15px;
  min-height: 128px;
  padding: 12px 14px;
}

.secret-input :deep(.el-textarea__inner) {
  overflow-wrap: break-word;
  word-break: normal;
  white-space: pre-wrap;
}

.dialog-footer :deep(.el-button) {
  border-radius: 8px;
  font-weight: 740;
  min-height: 40px;
  min-width: 86px;
}

@media (max-width: 760px) {
  .provider-row {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .status-toggle {
    justify-content: space-between;
    min-height: 32px;
    padding-bottom: 0;
  }

  .models-row {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .auto-fetch-link {
    justify-self: start;
  }

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

  .model-picker-toolbar {
    align-items: stretch;
    display: grid;
    grid-template-columns: 1fr;
  }

  .model-count,
  .model-action-links {
    white-space: normal;
  }

  .model-checkbox-list {
    grid-template-columns: 1fr;
  }

  :global(.channel-dialog .el-dialog__header) {
    padding: 24px 22px 10px;
  }

  :global(.channel-dialog .el-dialog__body) {
    padding: 16px 22px 18px;
  }

  :global(.channel-dialog .el-dialog__footer) {
    padding: 16px 22px 22px;
  }
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

.channel-key-table {
  margin-bottom: 8px;
}

.hidden-submit {
  display: none;
}
</style>
