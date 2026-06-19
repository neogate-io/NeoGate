<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
import { Coin, PriceTag, Search, UserFilled, View } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getPricingTemplates, syncPricingTemplates } from '../../api/prices'
import { getAdminServicePolicy, saveAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import type { PricingTemplate } from '../../types/admin'
import { createConfirmAction } from '../../utils/confirm'
import { ApiError, readError } from '../../utils/errors'
import { formatDateTime, formatMicrosPerMillion } from '../../utils/format'

const { locale, t } = useLocale()
const confirmDialog = createConfirmAction(() => t('cancel'))

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
  await withLoading(loading, async () => {
    try {
      const [policy, templates] = await Promise.all([
        getAdminServicePolicy(),
        getPricingTemplates()
      ])
      servicePolicy.value = policy
      pricingTemplates.value = templates
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
      pricingTemplates.value = await getPricingTemplates()
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
          {{ pricingTemplates.length.toLocaleString(locale) }}
        </span>
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

.reference-prices-table :deep(.cell) {
  font-size: 12px;
  line-height: 1.35;
  padding: 0 8px;
  white-space: normal;
  word-break: break-word;
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
