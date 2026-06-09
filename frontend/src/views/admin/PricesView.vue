<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { Edit, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  getPricingTemplates,
  getProviderModels,
  getProviderPrices,
  upsertProviderPrice
} from '../../api/prices'
import { getProviders } from '../../api/providers'
import { useLocale } from '../../composables/useLocale'
import type {
  PricingTemplate,
  ProviderModel,
  ProviderPrice,
  ProviderRecord
} from '../../types/admin'
import { readError } from '../../utils/errors'
import { formatMicrosPerMillion, microUsdToUsd, usdToMicroUsd } from '../../utils/format'
import { derivedCacheReadPrice, findPricingTemplate, priceKey } from '../../utils/pricing'

type ProviderModelOption = {
  provider: string
  providerLabel: string
  model: string
}

type PriceRow = ProviderModelOption & {
  price?: ProviderPrice
  template?: PricingTemplate
}

const { t } = useLocale()
const providers = ref<ProviderRecord[]>([])
const providerModels = ref<ProviderModel[]>([])
const prices = ref<ProviderPrice[]>([])
const templates = ref<PricingTemplate[]>([])
const loading = ref(false)
const saving = ref(false)

const form = reactive({
  provider: '',
  model: '',
  inputUsdPerMillion: 0,
  outputUsdPerMillion: 0,
  cacheReadUsdPerMillion: 0,
  cacheWriteUsdPerMillion: null as number | null,
  enabled: true
})

const providerByCode = computed(() => {
  return new Map(providers.value.map((provider) => [provider.code, provider]))
})

const providerModelOptions = computed(() => {
  const options = new Map<string, ProviderModelOption>()

  for (const providerModel of providerModels.value) {
    if (!providerModel.enabled) continue
    const provider = providerByCode.value.get(providerModel.provider)
    addProviderModelOption(
      options,
      providerModel.provider,
      provider?.display_name ?? providerModel.provider,
      providerModel.model
    )
  }

  for (const price of prices.value) {
    const provider = providerByCode.value.get(price.provider)
    addProviderModelOption(
      options,
      price.provider,
      provider?.display_name ?? price.provider,
      price.model
    )
  }

  return Array.from(options.values()).sort((left, right) => {
    const providerCompare = left.providerLabel.localeCompare(right.providerLabel)
    return providerCompare === 0 ? left.model.localeCompare(right.model) : providerCompare
  })
})

const providerOptions = computed(() => {
  const providersByCode = new Map<string, { value: string; label: string }>()
  for (const option of providerModelOptions.value) {
    providersByCode.set(option.provider, {
      value: option.provider,
      label: option.providerLabel
    })
  }
  return Array.from(providersByCode.values())
})

const modelOptions = computed(() => {
  return providerModelOptions.value.filter((option) => option.provider === form.provider)
})

const priceByModel = computed(() => {
  return new Map(prices.value.map((price) => [priceKey(price.provider, price.model), price]))
})

const rows = computed<PriceRow[]>(() => {
  return providerModelOptions.value
    .map((option) => ({
      ...option,
      price: priceByModel.value.get(priceKey(option.provider, option.model)),
      template: findPricingTemplate(templates.value, option.provider, option.model)
    }))
    .sort((left, right) => {
      if (Boolean(left.price) !== Boolean(right.price)) {
        return left.price ? 1 : -1
      }
      const providerCompare = left.providerLabel.localeCompare(right.providerLabel)
      return providerCompare === 0 ? left.model.localeCompare(right.model) : providerCompare
    })
})

function addProviderModelOption(
  options: Map<string, ProviderModelOption>,
  provider: string,
  providerLabel: string,
  model: string
) {
  const trimmed = model.trim()
  if (!trimmed) return
  const key = priceKey(provider, trimmed)
  if (!options.has(key)) {
    options.set(key, { provider, providerLabel, model: trimmed })
  }
}

function priceRowKey(row: PriceRow) {
  return priceKey(row.provider, row.model)
}

function formatCacheWritePrice(row: PriceRow) {
  if (!row.price) return '-'
  if (
    row.template &&
    (row.template.cache_write_price_usd_micros === undefined ||
      row.template.cache_write_price_usd_micros === null)
  ) {
    return t('noExtraCacheWriteFee')
  }
  return row.price.cache_write_price_usd_micros === undefined ||
    row.price.cache_write_price_usd_micros === null
    ? t('noExtraCacheWriteFee')
    : formatMicrosPerMillion(row.price.cache_write_price_usd_micros)
}

async function load() {
  loading.value = true
  try {
    const [fetchedProviders, fetchedProviderModels, fetchedPrices, fetchedTemplates] =
      await Promise.all([
        getProviders(),
        getProviderModels(),
        getProviderPrices(),
        getPricingTemplates()
      ])
    providers.value = fetchedProviders
    providerModels.value = fetchedProviderModels
    prices.value = fetchedPrices
    templates.value = fetchedTemplates
    selectInitialModel()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

function selectInitialModel() {
  const firstMissing = rows.value.find((row) => !row.price)
  const first = firstMissing ?? rows.value[0]
  if (first) {
    selectRow(first)
  }
}

function selectRow(row: PriceRow) {
  form.provider = row.provider
  form.model = row.model
  applyExistingPrice()
}

function handleProviderChange() {
  form.model = modelOptions.value[0]?.model ?? ''
  applyExistingPrice()
}

function applyExistingPrice() {
  const price = priceByModel.value.get(priceKey(form.provider, form.model))
  const template = findPricingTemplate(templates.value, form.provider, form.model)
  const inputPrice = price?.input_price_usd_micros ?? template?.input_price_usd_micros ?? 0
  const cacheWritePrice = template
    ? template.cache_write_price_usd_micros
    : (price?.cache_write_price_usd_micros ?? inputPrice)
  form.inputUsdPerMillion = microUsdToUsd(inputPrice)
  form.outputUsdPerMillion = microUsdToUsd(
    price?.output_price_usd_micros ?? template?.output_price_usd_micros ?? 0
  )
  form.cacheReadUsdPerMillion = microUsdToUsd(
    price?.cache_read_price_usd_micros ??
      template?.cache_read_price_usd_micros ??
      derivedCacheReadPrice(inputPrice)
  )
  form.cacheWriteUsdPerMillion =
    cacheWritePrice === undefined || cacheWritePrice === null
      ? template
        ? null
        : microUsdToUsd(inputPrice)
      : microUsdToUsd(cacheWritePrice)
  form.enabled = price?.enabled ?? true
}

async function savePrice() {
  if (!form.provider || !form.model) {
    ElMessage.warning(t('priceModelRequired'))
    return
  }

  saving.value = true
  try {
    await upsertProviderPrice({
      provider: form.provider,
      model: form.model,
      input_price_usd_micros: usdToMicroUsd(form.inputUsdPerMillion),
      output_price_usd_micros: usdToMicroUsd(form.outputUsdPerMillion),
      cache_read_price_usd_micros: usdToMicroUsd(form.cacheReadUsdPerMillion),
      cache_write_price_usd_micros:
        form.cacheWriteUsdPerMillion === null ? null : usdToMicroUsd(form.cacheWriteUsdPerMillion),
      enabled: form.enabled
    })
    ElMessage.success(t('priceSaved'))
    await load()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<template>
  <section class="grid admin-page-view">
    <el-form class="inline-admin-form price-editor-form" :model="form" label-position="top">
      <el-form-item :label="t('provider')">
        <el-select v-model="form.provider" filterable @change="handleProviderChange">
          <el-option
            v-for="provider in providerOptions"
            :key="provider.value"
            :label="provider.label"
            :value="provider.value"
          />
        </el-select>
      </el-form-item>
      <el-form-item :label="t('model')">
        <el-select
          v-model="form.model"
          filterable
          :disabled="!form.provider"
          @change="applyExistingPrice"
        >
          <el-option
            v-for="option in modelOptions"
            :key="option.model"
            :label="option.model"
            :value="option.model"
          />
        </el-select>
      </el-form-item>
      <el-form-item :label="t('inputPrice')">
        <el-input-number v-model="form.inputUsdPerMillion" :min="0" :step="0.01" />
      </el-form-item>
      <el-form-item :label="t('outputPrice')">
        <el-input-number v-model="form.outputUsdPerMillion" :min="0" :step="0.01" />
      </el-form-item>
      <el-form-item :label="t('cacheReadPrice')">
        <el-input-number v-model="form.cacheReadUsdPerMillion" :min="0" :step="0.01" />
      </el-form-item>
      <el-form-item :label="t('cacheWritePrice')">
        <el-input-number
          v-if="form.cacheWriteUsdPerMillion !== null"
          v-model="form.cacheWriteUsdPerMillion"
          :min="0"
          :step="0.01"
        />
        <el-tag v-else type="info">{{ t('noExtraCacheWriteFee') }}</el-tag>
      </el-form-item>
      <el-form-item :label="t('enabled')">
        <el-switch v-model="form.enabled" />
      </el-form-item>
      <el-button
        class="admin-action-button price-save-action"
        type="primary"
        :icon="Select"
        :loading="saving"
        @click="savePrice"
      >
        {{ t('save') }}
      </el-button>
    </el-form>

    <div class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table price-table"
        :data="rows"
        :row-key="priceRowKey"
        stripe
      >
        <el-table-column prop="providerLabel" :label="t('provider')" min-width="150" />
        <el-table-column prop="model" :label="t('model')" min-width="220" />
        <el-table-column :label="t('inputPrice')" min-width="150">
          <template #default="{ row }">
            {{ formatMicrosPerMillion(row.price?.input_price_usd_micros) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('outputPrice')" min-width="150">
          <template #default="{ row }">
            {{ formatMicrosPerMillion(row.price?.output_price_usd_micros) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('cacheReadPrice')" min-width="160">
          <template #default="{ row }">
            {{ formatMicrosPerMillion(row.price?.cache_read_price_usd_micros) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('cacheWritePrice')" min-width="160">
          <template #default="{ row }">
            {{ formatCacheWritePrice(row) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('status')" min-width="120">
          <template #default="{ row }">
            <el-tag v-if="!row.price" type="warning">{{ t('priceMissing') }}</el-tag>
            <el-tag v-else :type="row.price.enabled ? 'success' : 'info'">
              {{ row.price.enabled ? t('enabled') : t('disabled') }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="132" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button class="admin-action-button" :icon="Edit" @click="selectRow(row)">
                {{ row.price ? t('edit') : t('configure') }}
              </el-button>
            </div>
          </template>
        </el-table-column>
      </el-table>
    </div>
  </section>
</template>
