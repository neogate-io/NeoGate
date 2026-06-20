<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
import { Coin, PriceTag, Search, UserFilled, View } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getModelReferenceCatalog, syncPricingTemplates } from '../../api/prices'
import { getAdminServicePolicy, saveAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import type { ModelReferenceCatalogRecord } from '../../types/admin'
import { createConfirmAction } from '../../utils/confirm'
import { ApiError, readError } from '../../utils/errors'
import { formatDateTime, formatMicrosPerMillion } from '../../utils/format'

const { locale, t } = useLocale()
const confirmDialog = createConfirmAction(() => t('cancel'))

const loading = ref(false)
const servicePolicy = ref<ServicePolicy | null>(null)
const modelReferenceCatalog = ref<ModelReferenceCatalogRecord[]>([])
const servicePolicySaving = ref(false)
const syncingTemplates = ref(false)
const referencePricesDialogOpen = ref(false)
const referencePriceSearch = ref('')

const servicePolicyEditable = computed(() => servicePolicy.value?.service_mode === 'internal')
const referencePricesLastUpdated = computed(() => {
  const latest = modelReferenceCatalog.value.reduce<number | null>((max, template) => {
    const time = new Date(template.updated_at).getTime()
    if (Number.isNaN(time)) return max
    return max == null ? time : Math.max(max, time)
  }, null)

  return latest == null
    ? t('referencePricesNeverSynced')
    : formatDateTime(new Date(latest).toISOString(), locale.value)
})
const sortedPricingTemplates = computed(() => {
  return [...modelReferenceCatalog.value].sort((left, right) => {
    const providerCompare = left.provider.localeCompare(right.provider)
    return providerCompare || left.model.localeCompare(right.model)
  })
})
const filteredPricingTemplates = computed(() => {
  const keyword = referencePriceSearch.value.trim().toLowerCase()
  if (!keyword) return sortedPricingTemplates.value

  return sortedPricingTemplates.value.filter((template) => {
    return [template.provider, template.model, template.display_name, template.source].some(
      (value) => value.toLowerCase().includes(keyword)
    )
  })
})
const creditRequiredDescription = computed(() => {
  if (!servicePolicyEditable.value) return t('creditRequiredPaidDescription')
  return servicePolicy.value?.credit_required
    ? t('creditRequiredEnabledDescription')
    : t('creditRequiredDisabledDescription')
})
const registrationDescription = computed(() => {
  if (!servicePolicy.value) return ''
  if (!servicePolicy.value.registration_enabled) return t('registrationDisabledDescription')
  return servicePolicy.value.service_mode === 'paid'
    ? t('registrationPaidEnabledDescription')
    : t('registrationInternalEnabledDescription')
})

function formatSyncCount(value: number) {
  return value.toLocaleString('en-US')
}

function pricingBasisLabel(template: ModelReferenceCatalogRecord) {
  return template.pricing_basis === 'image'
    ? t('billingMeterImageGeneration')
    : t('billingMeterToken')
}

function formatReferencePricePair(template: ModelReferenceCatalogRecord) {
  if (template.pricing_basis === 'image') {
    return `${formatMicrosPerMillion(template.unit_price_usd_micros)} / ${t('perImage')}`
  }
  return `${formatMicrosPerMillion(template.input_price_usd_micros)} / ${formatMicrosPerMillion(
    template.output_price_usd_micros
  )}`
}

function formatCachePricePair(template: ModelReferenceCatalogRecord) {
  if (template.pricing_basis === 'image') return '-'
  const cacheRead =
    template.cache_read_price_usd_micros == null
      ? '$0'
      : formatMicrosPerMillion(template.cache_read_price_usd_micros)
  const cacheWrite =
    template.cache_write_price_usd_micros == null
      ? '$0'
      : formatMicrosPerMillion(template.cache_write_price_usd_micros)
  return `${cacheRead} / ${cacheWrite}`
}

function referencePricesSyncedMessage(result: { saved: number }) {
  if (locale.value === 'zh-CN') {
    return `${t('referencePricesSynced')}，已更新 ${formatSyncCount(result.saved)} 条模型参考价。配置渠道价格时可一键应用。`
  }

  return `${t('referencePricesSynced')}. Updated ${formatSyncCount(result.saved)} model reference prices. You can apply them when configuring channel prices.`
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

async function load() {
  await withLoading(loading, async () => {
    try {
      const [policy, catalog] = await Promise.all([
        getAdminServicePolicy(),
        getModelReferenceCatalog()
      ])
      servicePolicy.value = policy
      modelReferenceCatalog.value = catalog
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function saveServicePolicy() {
  const policy = servicePolicy.value
  if (!policy) return

  await withLoading(servicePolicySaving, async () => {
    try {
      servicePolicy.value = await saveAdminServicePolicy({
        credit_required: policy.credit_required,
        registration_enabled: policy.registration_enabled
      })
      ElMessage.success(t('servicePolicySaved'))
    } catch (err) {
      ElMessage.error(readError(err))
      servicePolicy.value = await getAdminServicePolicy().catch(() => servicePolicy.value)
    }
  })
}

async function syncReferencePrices() {
  const confirmed = await confirmDialog(
    referenceSyncConfirmContent(),
    t('syncReferencePricesConfirmTitle'),
    {
      confirmText: t('syncReferencePricesConfirmButton')
    }
  )
  if (!confirmed) return

  await withLoading(syncingTemplates, async () => {
    try {
      const result = await syncPricingTemplates()
      modelReferenceCatalog.value = await getModelReferenceCatalog()
      ElMessage.success(referencePricesSyncedMessage(result))
    } catch (err) {
      ElMessage.error(readReferenceSyncError(err))
    }
  })
}

onMounted(load)
</script>

<template>
  <section class="admin-settings-view other-settings-view">
    <div v-loading="loading" class="other-settings-grid">
      <section class="other-settings-card">
        <header class="admin-settings-section-header other-settings-card-header">
          <el-icon class="admin-settings-panel-icon"><Coin /></el-icon>
          <div class="other-settings-card-copy">
            <h3>{{ t('creditRequired') }}</h3>
            <p>{{ creditRequiredDescription }}</p>
          </div>
          <el-switch
            v-if="servicePolicy"
            v-model="servicePolicy.credit_required"
            class="other-settings-switch"
            :disabled="!servicePolicy || !servicePolicyEditable || servicePolicySaving"
            @change="saveServicePolicy"
          />
        </header>
      </section>

      <section class="other-settings-card">
        <header class="admin-settings-section-header other-settings-card-header">
          <el-icon class="admin-settings-panel-icon"><UserFilled /></el-icon>
          <div class="other-settings-card-copy">
            <h3>{{ t('registrationEnabled') }}</h3>
            <p>{{ registrationDescription }}</p>
          </div>
          <el-switch
            v-if="servicePolicy"
            v-model="servicePolicy.registration_enabled"
            class="other-settings-switch"
            :disabled="!servicePolicy || servicePolicySaving"
            @change="saveServicePolicy"
          />
        </header>
      </section>

      <section class="other-settings-card">
        <header class="admin-settings-section-header other-settings-card-header">
          <el-icon class="admin-settings-panel-icon"><PriceTag /></el-icon>
          <div class="other-settings-card-copy">
            <h3>{{ t('modelReferencePrices') }}</h3>
            <p>{{ t('syncReferencePricesConfirmIntro') }}</p>
            <p class="other-settings-meta">
              <span>{{ t('referencePricesLastUpdated') }}</span>
              <strong>{{ referencePricesLastUpdated }}</strong>
            </p>
          </div>
        </header>
        <div class="other-settings-actions">
          <el-button
            class="admin-action-button"
            :icon="View"
            @click="referencePricesDialogOpen = true"
          >
            {{ t('viewReferencePrices') }}
          </el-button>
          <el-button
            class="admin-action-button"
            type="primary"
            :loading="syncingTemplates"
            @click="syncReferencePrices"
          >
            {{ t('syncReferencePrices') }}
          </el-button>
        </div>
      </section>
    </div>

    <el-dialog
      v-model="referencePricesDialogOpen"
      class="reference-prices-dialog"
      :title="t('modelReferencePrices')"
      width="1120px"
    >
      <div class="reference-prices-toolbar">
        <el-input
          v-model="referencePriceSearch"
          clearable
          :prefix-icon="Search"
          :placeholder="t('referencePricesSearchPlaceholder')"
        />
        <span class="reference-result-count">
          {{ t('referencePricesResult') }}
          {{ filteredPricingTemplates.length.toLocaleString(locale) }} /
          {{ modelReferenceCatalog.length.toLocaleString(locale) }}
        </span>
      </div>
      <el-table
        class="admin-table reference-prices-table"
        :data="filteredPricingTemplates"
        max-height="62vh"
        stripe
      >
        <el-table-column prop="provider" :label="t('provider')" width="112" />
        <el-table-column prop="model" :label="t('modelName')" min-width="260" />
        <el-table-column :label="t('pricingBasis')" width="92" align="center" header-align="center">
          <template #default="{ row }">
            <span class="reference-meter-badge">{{ pricingBasisLabel(row) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          class-name="reference-price-column"
          min-width="180"
          align="right"
          header-align="right"
        >
          <template #header>
            <span class="reference-price-head-label">{{ t('inputOutputPriceShort') }}</span>
          </template>
          <template #default="{ row }">
            <span class="reference-price-value">{{ formatReferencePricePair(row) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          class-name="reference-price-column"
          min-width="180"
          align="right"
          header-align="right"
        >
          <template #header>
            <span class="reference-price-head-label">{{ t('cacheReadWritePriceShort') }}</span>
          </template>
          <template #default="{ row }">
            <span class="reference-price-value">{{ formatCachePricePair(row) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('source')" width="118">
          <template #default="{ row }">
            <span class="reference-source-text">{{ row.source }}</span>
          </template>
        </el-table-column>
        <template #empty>
          <el-empty :description="t('noData')" />
        </template>
      </el-table>
    </el-dialog>
  </section>
</template>

<style scoped>
:global(.other-settings-view.admin-settings-view) {
  align-items: flex-start;
}

.other-settings-grid {
  display: grid;
  gap: 16px;
  width: min(780px, 100%);
}

.other-settings-card {
  background: var(--admin-surface);
  border: 1px solid var(--admin-border);
  border-radius: var(--admin-radius);
  box-shadow: none;
  display: grid;
  gap: 16px;
  min-width: 0;
  padding: 20px 22px;
}

.other-settings-card-header {
  align-items: flex-start;
  grid-template-columns: 28px minmax(0, 1fr) auto;
}

.other-settings-card-copy {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.other-settings-card-copy h3 {
  color: var(--admin-text);
  font-size: 14px;
  font-weight: 760;
  line-height: 1.25;
  margin: 0;
}

.other-settings-card-copy p {
  color: var(--admin-text-muted);
  font-size: 13px;
  font-weight: 560;
  line-height: 1.6;
  margin: 0;
}

.other-settings-switch {
  margin-top: 1px;
}

.other-settings-meta {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 4px;
}

.other-settings-meta strong {
  color: var(--admin-text);
  font-weight: 720;
}

.other-settings-actions {
  border-top: 1px solid var(--admin-border-soft);
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  justify-content: flex-end;
  padding-top: 16px;
}

:global(.reference-prices-dialog) {
  max-width: calc(100vw - 32px);
}

.reference-prices-table {
  width: 100%;
}

.reference-prices-toolbar {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
  margin-bottom: 12px;
}

.reference-prices-toolbar :deep(.el-input) {
  max-width: 360px;
}

.reference-result-count {
  color: #697586;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 620;
}

.reference-prices-table :deep(.el-table__cell) {
  padding: 6px 0;
}

.reference-prices-table :deep(.el-table__header .cell) {
  color: #4e5969;
  font-size: 12px;
  font-weight: 720;
}

.reference-prices-table :deep(.reference-price-column .cell) {
  text-align: right;
}

.reference-price-head-label {
  display: flex;
  justify-content: flex-end;
  min-width: 0;
  white-space: nowrap;
  width: 100%;
}

.reference-prices-table :deep(.cell) {
  font-size: 12px;
  line-height: 1.35;
  padding: 0 10px;
  white-space: normal;
  word-break: break-word;
}

.reference-price-value {
  color: #263242;
  display: block;
  font-variant-numeric: tabular-nums;
  font-weight: 620;
  text-align: right;
  white-space: nowrap;
}

.reference-meter-badge {
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
  padding: 4px 10px;
}

.reference-source-text {
  color: #596579;
  display: inline-block;
  font-weight: 620;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 640px) {
  .other-settings-card {
    padding: 18px 16px;
  }

  .other-settings-card-header {
    grid-template-columns: 28px minmax(0, 1fr);
  }

  .other-settings-switch {
    grid-column: 2;
    justify-self: start;
    margin-top: 4px;
  }

  .other-settings-actions {
    justify-content: stretch;
  }

  .other-settings-actions .el-button {
    flex: 1 1 0;
    min-width: 0;
  }
}
</style>
