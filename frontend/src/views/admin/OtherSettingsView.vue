<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { getPricingTemplates, syncPricingTemplates } from '../../api/prices'
import { getAdminServicePolicy, saveAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import { useLocale } from '../../composables/useLocale'
import type { PricingTemplate } from '../../types/admin'
import { ApiError, readError } from '../../utils/errors'
import { formatDateTime } from '../../utils/format'

const { locale, t } = useLocale()

const loading = ref(false)
const servicePolicy = ref<ServicePolicy | null>(null)
const pricingTemplates = ref<PricingTemplate[]>([])
const servicePolicySaving = ref(false)
const syncingTemplates = ref(false)

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
const creditRequiredDescription = computed(() => {
  if (!servicePolicyEditable.value) return t('creditRequiredPaidDescription')
  return servicePolicy.value?.credit_required
    ? t('creditRequiredEnabledDescription')
    : t('creditRequiredDisabledDescription')
})

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
          <h3>{{ t('creditRequired') }}</h3>
          <p>{{ creditRequiredDescription }}</p>
        </div>
      </div>

      <el-form class="settings-form" label-position="top">
        <el-form-item>
          <el-switch
            v-if="servicePolicy"
            v-model="servicePolicy.credit_required"
            :disabled="!servicePolicy || !servicePolicyEditable"
          />
          <span class="settings-inline-hint">
            {{
              servicePolicyEditable
                ? servicePolicy?.credit_required
                  ? t('enabled')
                  : t('disabled')
                : t('creditRequiredPaidHint')
            }}
          </span>
        </el-form-item>
        <el-button
          v-if="servicePolicyEditable"
          type="primary"
          :loading="servicePolicySaving"
          @click="saveServicePolicy"
        >
          {{ t('save') }}
        </el-button>
      </el-form>
    </div>

    <div class="admin-settings-panel">
      <div class="settings-panel-header">
        <div>
          <h3>{{ t('syncReferencePrices') }}</h3>
          <p>{{ t('syncReferencePricesConfirmIntro') }}</p>
          <p class="reference-sync-meta">
            <span>{{ t('referencePricesLastUpdated') }}</span>
            <strong>{{ referencePricesLastUpdated }}</strong>
          </p>
        </div>
      </div>
      <div>
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

.settings-inline-hint {
  color: #697586;
  font-size: 13px;
  margin-left: 10px;
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
</style>
