<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
import { Search, View } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { getPricingTemplates, syncPricingTemplates } from '../../api/prices'
import { getAdminServicePolicy, saveAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import { useLocale } from '../../composables/useLocale'
import type { PricingTemplate } from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { formatDateTime, formatMicrosPerMillion } from '../../utils/format'

const { locale, t } = useLocale()

const loading = ref(false)
const servicePolicy = ref<ServicePolicy | null>(null)
const pricingTemplates = ref<PricingTemplate[]>([])
const servicePolicySaving = ref(false)
const syncingTemplates = ref(false)
const referencePricesDialogOpen = ref(false)
const referencePriceSearch = ref('')

const servicePolicyEditable = computed(() => servicePolicy.value?.service_mode === 'internal')
const referencePricesLastUpdated = computed(() => {
  const latest = pricingTemplates.value.reduce<number | null>((max, template) => {
    const time = new Date(template.updated_at).getTime()
    if (Number.isNaN(time)) return max
    return max == null ? time : Math.max(max, time)
  }, null)

  return latest == null
    ? t('referencePricesNeverSynced')
    : formatDateTime(new Date(latest).toISOString(), locale.value)
})
const sortedPricingTemplates = computed(() => {
  return [...pricingTemplates.value].sort((left, right) => {
    const providerCompare = left.provider.localeCompare(right.provider)
    return providerCompare || left.model.localeCompare(right.model)
  })
})
const filteredPricingTemplates = computed(() => {
  const keyword = referencePriceSearch.value.trim().toLowerCase()
  if (!keyword) return sortedPricingTemplates.value

  return sortedPricingTemplates.value.filter((template) => {
    return [template.provider, template.model, template.source].some((value) =>
      value.toLowerCase().includes(keyword)
    )
  })
})
const creditRequiredDescription = computed(() => {
  if (!servicePolicyEditable.value) return t('creditRequiredPaidDescription')
  return servicePolicy.value?.credit_required
    ? t('creditRequiredEnabledDescription')
    : t('creditRequiredDisabledDescription')
})

function formatSyncCount(value: number) {
  return value.toLocaleString('en-US')
}

function formatCacheWritePrice(template: PricingTemplate) {
  return template.cache_write_price_usd_micros == null
    ? t('noExtraCacheWriteFee')
    : formatMicrosPerMillion(template.cache_write_price_usd_micros)
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

async function load() {
  loading.value = true
  try {
    const [policy, templates] = await Promise.all([getAdminServicePolicy(), getPricingTemplates()])
    servicePolicy.value = policy
    pricingTemplates.value = templates
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function saveServicePolicy() {
  if (!servicePolicy.value || !servicePolicyEditable.value) return

  servicePolicySaving.value = true
  try {
    servicePolicy.value = await saveAdminServicePolicy({
      credit_required: servicePolicy.value.credit_required
    })
    ElMessage.success(t('servicePolicySaved'))
  } catch (err) {
    ElMessage.error(readError(err))
    servicePolicy.value = await getAdminServicePolicy().catch(() => servicePolicy.value)
  } finally {
    servicePolicySaving.value = false
  }
}

async function syncReferencePrices() {
  try {
    await ElMessageBox.confirm(
      referenceSyncConfirmContent(),
      t('syncReferencePricesConfirmTitle'),
      {
        confirmButtonText: t('syncReferencePricesConfirmButton'),
        cancelButtonText: t('cancel'),
        customClass: 'reference-sync-confirm'
      }
    )
  } catch {
    return
  }

  syncingTemplates.value = true
  try {
    const result = await syncPricingTemplates()
    pricingTemplates.value = await getPricingTemplates()
    ElMessage.success(referencePricesSyncedMessage(result))
  } catch (err) {
    ElMessage.error(readReferenceSyncError(err))
  } finally {
    syncingTemplates.value = false
  }
}

onMounted(load)
</script>

<template>
  <section class="grid other-settings-view">
    <div v-loading="loading" class="admin-settings-panel">
      <div class="settings-panel-header">
        <div>
          <div class="settings-title-row">
            <h3>{{ t('creditRequired') }}</h3>
            <el-switch
              v-if="servicePolicy"
              v-model="servicePolicy.credit_required"
              :disabled="!servicePolicy || !servicePolicyEditable || servicePolicySaving"
              @change="saveServicePolicy"
            />
          </div>
          <p>{{ creditRequiredDescription }}</p>
        </div>
      </div>
    </div>

    <div class="admin-settings-panel">
      <div class="settings-panel-header">
        <div>
          <h3>{{ t('modelReferencePrices') }}</h3>
          <p>{{ t('syncReferencePricesConfirmIntro') }}</p>
          <p class="reference-sync-meta">
            <span>{{ t('referencePricesLastUpdated') }}</span>
            <strong>{{ referencePricesLastUpdated }}</strong>
          </p>
        </div>
      </div>
      <div class="reference-actions">
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
    </div>

    <el-dialog
      v-model="referencePricesDialogOpen"
      class="reference-prices-dialog"
      :title="t('modelReferencePrices')"
      width="960px"
    >
      <div class="reference-prices-toolbar">
        <el-input
          v-model="referencePriceSearch"
          clearable
          :prefix-icon="Search"
          :placeholder="t('referencePricesSearchPlaceholder')"
        />
      </div>
      <el-table
        class="admin-table reference-prices-table"
        :data="filteredPricingTemplates"
        max-height="62vh"
        stripe
      >
        <el-table-column prop="provider" :label="t('provider')" width="108" />
        <el-table-column prop="model" :label="t('model')" min-width="210" />
        <el-table-column :label="t('inputPrice')" min-width="116">
          <template #default="{ row }">
            {{ formatMicrosPerMillion(row.input_price_usd_micros) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('outputPrice')" min-width="116">
          <template #default="{ row }">
            {{ formatMicrosPerMillion(row.output_price_usd_micros) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('cacheReadPrice')" min-width="124">
          <template #default="{ row }">
            {{ formatMicrosPerMillion(row.cache_read_price_usd_micros) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('cacheWritePrice')" min-width="124">
          <template #default="{ row }">
            {{ formatCacheWritePrice(row) }}
          </template>
        </el-table-column>
        <el-table-column prop="source" :label="t('source')" width="108" />
      </el-table>
    </el-dialog>
  </section>
</template>

<style scoped>
.other-settings-view {
  align-content: start;
}

.admin-settings-panel {
  background: #ffffff;
  border: 1px solid #e5edf5;
  border-radius: 8px;
  display: grid;
  gap: 14px;
  padding: 18px;
}

.settings-panel-header h3 {
  color: #111827;
  font-size: 18px;
  font-weight: 800;
  margin: 0;
}

.settings-title-row {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.settings-panel-header p {
  color: #697586;
  font-size: 13px;
  font-weight: 560;
  line-height: 1.6;
  margin: 6px 0 0;
}

.settings-panel-header .reference-sync-meta {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 10px;
}

.reference-sync-meta span {
  color: #697586;
}

.reference-sync-meta strong {
  color: #111827;
  font-weight: 780;
}

.reference-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
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

:global(.reference-prices-dialog) {
  max-width: calc(100vw - 32px);
}

.reference-prices-table {
  width: 100%;
}

.reference-prices-toolbar {
  margin-bottom: 12px;
  max-width: 360px;
}

.reference-prices-table :deep(.el-table__cell) {
  padding: 6px 0;
}

.reference-prices-table :deep(.cell) {
  font-size: 12px;
  line-height: 1.35;
  padding: 0 8px;
  white-space: normal;
  word-break: break-word;
}
</style>
