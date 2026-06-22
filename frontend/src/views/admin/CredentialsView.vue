<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  Calendar,
  Clock,
  Delete,
  Refresh,
  SwitchButton,
  Upload,
  Warning
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  deleteCredential,
  disableCredential,
  enableCredential,
  getCredentials,
  refreshCredential as refreshCredentialApi,
  uploadCredentialFile
} from '../../api/credentials'
import ProviderIcon from '../../components/common/ProviderIcon.vue'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type { Credential, CredentialQuotaWindow } from '../../types/admin'
import { createConfirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import { formatCompactDateTime } from '../../utils/format'

const { t } = useLocale()
const confirmDialog = createConfirmAction(() => t('cancel'))
const credentials = ref<Credential[]>([])
const loading = ref(false)
const uploading = ref(false)
const refreshingAll = ref(false)
const refreshingIds = useReactiveSet<number>()
const togglingIds = useReactiveSet<number>()
const deletingIds = useReactiveSet<number>()
const selectedIds = useReactiveSet<number>()
const uploadInput = ref<HTMLInputElement | null>(null)

const sortedCredentials = computed(() =>
  [...credentials.value].sort((left, right) => {
    if (left.enabled !== right.enabled) return left.enabled ? -1 : 1
    const leftTime = left.updated_at ? new Date(left.updated_at).getTime() : 0
    const rightTime = right.updated_at ? new Date(right.updated_at).getTime() : 0
    return rightTime - leftTime
  })
)
const selectedCount = selectedIds.size

async function loadCredentials() {
  await withLoading(loading, async () => {
    try {
      credentials.value = await getCredentials()
      pruneSelectedCredentials()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function refreshEnabledCredentials() {
  const ids = credentials.value
    .filter((credential) => credential.enabled && isOpenAICredential(credential))
    .map((credential) => credential.id)
  if (ids.length === 0) return
  await withLoading(refreshingAll, () => Promise.all(ids.map((id) => refreshCredential(id, false))))
}

async function refreshCredential(id: number, notify = true) {
  await refreshingIds.withItem(id, async () => {
    try {
      const updated = await refreshCredentialApi(id)
      mergeCredential(updated)
    } catch (err) {
      if (notify) ElMessage.error(readError(err))
    }
  })
}

async function toggleCredential(credential: Credential) {
  await togglingIds.withItem(credential.id, async () => {
    try {
      const updated = credential.enabled
        ? await disableCredential(credential.id)
        : await enableCredential(credential.id)
      mergeCredential(updated)
      ElMessage.success(credential.enabled ? t('credentialDisabled') : t('credentialEnabled'))
      if (updated.enabled) {
        await refreshCredential(updated.id, false)
      }
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function removeCredential(credential: Credential) {
  const confirmed = await confirmDialog(t('credentialDeleteConfirm'), t('delete'), {
    confirmText: t('delete'),
    danger: true,
    type: 'warning'
  })
  if (!confirmed) return

  await deletingIds.withItem(credential.id, async () => {
    try {
      await deleteCredential(credential.id)
      credentials.value = credentials.value.filter((item) => item.id !== credential.id)
      selectedIds.remove(credential.id)
      ElMessage.success(t('credentialDeleted'))
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function uploadCredential(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return

  await withLoading(uploading, async () => {
    try {
      const result = await uploadCredentialFile(file)
      const imported = result.imported.length
      const failed = result.failed.length
      for (const credential of result.imported) {
        mergeCredential(credential)
      }
      const message =
        failed > 0
          ? `${t('credentialUploadDone')} ${imported}, ${t('credentialUploadFailed')} ${failed}`
          : `${t('credentialUploadDone')} ${imported}`
      ElMessage.success(message)
      if (failed > 0) {
        ElMessage.warning(result.failed.map((item) => `${item.filename}: ${item.error}`).join('\n'))
      }
      await loadCredentials()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

function openCredentialUpload() {
  if (uploading.value) return
  uploadInput.value?.click()
}

function mergeCredential(updated: Credential) {
  const index = credentials.value.findIndex((credential) => credential.id === updated.id)
  if (index === -1) {
    credentials.value = [...credentials.value, updated]
    return
  }
  const next = [...credentials.value]
  next[index] = updated
  credentials.value = next
}

function pruneSelectedCredentials() {
  const availableIds = new Set(credentials.value.map((credential) => credential.id))
  selectedIds.retain((id) => availableIds.has(id))
}

function isCredentialSelected(id: number) {
  return selectedIds.has(id)
}

function toggleCredentialSelection(id: number, checked: string | number | boolean) {
  selectedIds.toggle(id, checked === true || checked === 'true' || checked === 1 || checked === '1')
}

function toggleCredentialSelected(id: number) {
  toggleCredentialSelection(id, !isCredentialSelected(id))
}

function formatPercent(window?: CredentialQuotaWindow | null) {
  if (!window?.percent && window?.percent !== 0) return '--'
  return `${Math.round(window.percent)}%`
}

function quotaTrackWidth(window?: CredentialQuotaWindow | null) {
  if (!window?.percent && window?.percent !== 0) return '0%'
  return `${Math.max(0, Math.min(100, window.percent))}%`
}

function credentialTitle(credential: Credential) {
  return (
    credential.identity_label ||
    credential.email ||
    credential.account_id ||
    credential.filename ||
    t('credentialUnknownIdentity')
  )
}

function isOpenAICredential(credential: Credential) {
  return credential.provider.trim().toLowerCase() === 'openai'
}

function credentialPlanLabel(credential: Credential) {
  const plan = credential.quota?.plan?.trim()
  if (!plan || plan.toLowerCase() === 'openai') return 'FREE'
  return plan.toUpperCase()
}

function formatResetLabel(value?: string | null) {
  if (!value) return `${t('credentialResetAt')}: -`
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return `${t('credentialResetAt')}: -`

  const diffMinutes = Math.max(0, Math.round((date.getTime() - Date.now()) / 60000))
  const relative =
    diffMinutes < 60
      ? `${diffMinutes}${t('minuteShort')}`
      : diffMinutes < 1440
        ? `${Math.round(diffMinutes / 60)}${t('hourShort')}`
        : `${Math.round(diffMinutes / 1440)}${t('dayShort')}`
  const dayLabel = isTomorrow(date) ? t('tomorrow') : formatShortDate(date)
  const time = `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
  return `${t('credentialResetAt')}: ${relative} (${dayLabel} ${time})`
}

function isTomorrow(date: Date) {
  const tomorrow = new Date()
  tomorrow.setDate(tomorrow.getDate() + 1)
  return (
    date.getFullYear() === tomorrow.getFullYear() &&
    date.getMonth() === tomorrow.getMonth() &&
    date.getDate() === tomorrow.getDate()
  )
}

function formatShortDate(date: Date) {
  return `${date.getMonth() + 1}/${date.getDate()}`
}

async function bootstrap() {
  await loadCredentials()
  await refreshEnabledCredentials()
}

void bootstrap()
</script>

<template>
  <section class="grid credential-page">
    <div class="table-toolbar admin-page-toolbar">
      <span v-if="selectedCount > 0" class="credential-selection-count">
        {{ t('credentialSelectedCount') }} {{ selectedCount }}
      </span>
      <input
        ref="uploadInput"
        class="credential-upload-input"
        accept=".json,.zip,application/json,application/zip"
        type="file"
        @change="uploadCredential"
      />
      <div class="admin-page-toolbar-actions">
        <el-button
          class="admin-action-button"
          type="primary"
          :icon="Upload"
          :loading="uploading"
          @click="openCredentialUpload"
        >
          {{ t('credentialUploadFile') }}
        </el-button>
        <el-button
          class="admin-action-button"
          :icon="Refresh"
          :loading="refreshingAll || loading"
          @click="refreshEnabledCredentials"
        >
          {{ t('refreshAll') }}
        </el-button>
      </div>
    </div>

    <div v-loading="loading" class="credential-grid">
      <div v-if="!loading && sortedCredentials.length === 0" class="credential-empty">
        <el-icon><Warning /></el-icon>
        <p>{{ t('credentialNoFiles') }}</p>
      </div>

      <article
        v-for="credential in sortedCredentials"
        :key="credential.id"
        class="credential-card"
        :class="{
          'is-disabled': !credential.enabled,
          'is-selected': isCredentialSelected(credential.id)
        }"
      >
        <button
          class="credential-select"
          type="button"
          :aria-label="credentialTitle(credential)"
          :aria-pressed="isCredentialSelected(credential.id)"
          :class="{ 'is-checked': isCredentialSelected(credential.id) }"
          @click="toggleCredentialSelected(credential.id)"
        >
          <span />
        </button>
        <div class="credential-card-header">
          <span class="credential-provider-slot">
            <ProviderIcon class="credential-provider-icon" :provider="credential.provider" />
          </span>
          <div class="credential-identity">
            <div class="credential-title-block">
              <strong>{{ credentialTitle(credential) }}</strong>
              <el-tag
                v-if="isOpenAICredential(credential)"
                class="credential-plan-tag"
                :class="{ 'is-disabled': !credential.enabled }"
                effect="plain"
                round
              >
                {{ credentialPlanLabel(credential) }}
              </el-tag>
              <el-tag v-else class="credential-plan-tag" effect="plain" round>
                {{ credential.provider }}
              </el-tag>
            </div>
          </div>
        </div>

        <div v-if="isOpenAICredential(credential)" class="quota-grid">
          <div class="quota-section primary">
            <div class="quota-row">
              <span class="quota-label-main">
                <el-icon><Clock /></el-icon>
                {{ t('credentialFiveHourQuota') }}
              </span>
              <strong class="quota-value is-primary">{{
                formatPercent(credential.quota?.five_hour)
              }}</strong>
            </div>
            <div class="quota-track">
              <span
                class="quota-track-fill"
                :style="{ width: quotaTrackWidth(credential.quota?.five_hour) }"
              />
            </div>
            <p class="quota-reset">{{ formatResetLabel(credential.quota?.five_hour?.reset_at) }}</p>
          </div>

          <div class="quota-section secondary">
            <div class="quota-row">
              <span class="quota-label-main">
                <el-icon><Calendar /></el-icon>
                {{ t('credentialWeeklyQuota') }}
              </span>
              <strong class="quota-value is-secondary">{{
                formatPercent(credential.quota?.weekly)
              }}</strong>
            </div>
            <div class="quota-track quota-track-secondary">
              <span
                class="quota-track-fill"
                :style="{ width: quotaTrackWidth(credential.quota?.weekly) }"
              />
            </div>
            <p class="quota-reset">{{ formatResetLabel(credential.quota?.weekly?.reset_at) }}</p>
          </div>
        </div>

        <div class="credential-footer">
          <span class="credential-updated-at">{{
            formatCompactDateTime(credential.updated_at || credential.last_refresh)
          }}</span>
          <div class="credential-actions">
            <el-tooltip
              :content="credential.enabled ? t('disable') : t('enable')"
              placement="top"
              :show-after="600"
            >
              <el-button
                circle
                class="credential-icon-button"
                :icon="SwitchButton"
                :loading="togglingIds.has(credential.id)"
                @click="toggleCredential(credential)"
              />
            </el-tooltip>
            <el-tooltip :content="t('refreshQuota')" placement="top" :show-after="600">
              <el-button
                v-if="isOpenAICredential(credential)"
                circle
                class="credential-icon-button"
                :icon="Refresh"
                :loading="refreshingIds.has(credential.id)"
                @click="refreshCredential(credential.id)"
              />
            </el-tooltip>
            <el-tooltip :content="t('delete')" placement="top" :show-after="600">
              <el-button
                circle
                class="credential-icon-button is-danger"
                :icon="Delete"
                :loading="deletingIds.has(credential.id)"
                @click="removeCredential(credential)"
              />
            </el-tooltip>
          </div>
        </div>
      </article>
    </div>
  </section>
</template>

<style scoped>
.credential-page {
  gap: 16px;
}

.credential-upload-input {
  display: none;
}

.credential-selection-count {
  color: #64748b;
  font-size: 13px;
  font-weight: 620;
}

.credential-grid {
  display: grid;
  gap: 14px;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  justify-content: start;
}

.credential-empty {
  align-items: center;
  background: #fff;
  border: 1px dashed var(--admin-border);
  border-radius: 8px;
  color: var(--admin-text-muted);
  display: grid;
  gap: 10px;
  justify-items: center;
  min-height: 220px;
  padding: 28px;
}

.credential-empty .el-icon {
  font-size: 28px;
}

.credential-card {
  background: #ffffff;
  border: 1px solid var(--admin-border);
  border-radius: 8px;
  box-shadow: var(--admin-shadow);
  display: grid;
  gap: 12px;
  min-height: 0;
  overflow: hidden;
  padding: 12px;
  position: relative;
  transition:
    background-color 160ms ease,
    border-color 160ms ease,
    box-shadow 160ms ease;
  width: 100%;
}

.credential-card::before {
  background: var(--brand-blue);
  content: '';
  inset: 0 auto 0 0;
  opacity: 0;
  position: absolute;
  transition: opacity 160ms ease;
  width: 3px;
}

.credential-card:hover {
  background: #fbfdff;
  border-color: #c8d4e2;
  box-shadow: 0 8px 20px rgba(15, 23, 42, 0.055);
}

.credential-card.is-selected {
  border-color: var(--brand-blue);
  box-shadow: var(--admin-focus-ring);
}

.credential-card.is-selected::before {
  opacity: 1;
}

.credential-card.is-disabled {
  background: #f8fafc;
  opacity: 0.78;
}

.credential-card-header {
  align-items: center;
  display: grid;
  gap: 8px;
  grid-template-columns: 22px minmax(0, 1fr);
  min-width: 0;
}

.credential-select {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  cursor: pointer;
  display: inline-flex;
  height: 20px;
  opacity: 0;
  padding: 0;
  position: absolute;
  right: 8px;
  top: 8px;
  transition: none;
  width: 20px;
  z-index: 2;
}

.credential-card:hover .credential-select,
.credential-card.is-selected .credential-select,
.credential-select:focus-visible {
  opacity: 1;
}

.credential-select span {
  background: #ffffff;
  border: 2px solid #94a3b8;
  border-radius: 6px;
  box-shadow: inset 0 0 0 2px #ffffff;
  display: block;
  height: 19px;
  position: relative;
  transition: none;
  width: 19px;
}

.credential-select:hover span {
  border-color: #64748b;
}

.credential-select.is-checked span {
  background: #ffffff;
  border-color: var(--brand-blue);
}

.credential-select.is-checked span::after {
  border: solid var(--brand-blue);
  border-width: 0 2px 2px 0;
  content: '';
  height: 8px;
  left: 5px;
  position: absolute;
  top: 2px;
  transform: rotate(45deg);
  width: 4px;
}

.credential-select:focus-visible span {
  outline: 2px solid var(--brand-blue-border);
  outline-offset: 2px;
}

.credential-identity {
  min-width: 0;
}

.credential-provider-slot {
  align-items: center;
  display: inline-flex;
  height: 22px;
  justify-self: center;
  width: 22px;
}

.credential-provider-icon {
  height: 22px;
  width: 22px;
}

.credential-title-block {
  align-items: center;
  display: flex;
  gap: 8px;
  min-width: 0;
}

.credential-title-block strong {
  color: #0f172a;
  flex: 0 1 auto;
  font-size: 14px;
  font-weight: 500;
  line-height: 1.2;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.quota-reset,
.credential-updated-at {
  color: #6b7a90;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.credential-plan-tag.el-tag {
  --el-tag-bg-color: var(--brand-blue-soft);
  --el-tag-border-color: var(--brand-blue-border);
  --el-tag-text-color: var(--brand-blue);
  animation: none;
  font-size: 12px;
  font-weight: 500;
  flex: 0 0 auto;
  height: 26px;
  min-width: 54px;
  padding: 0 11px;
  transition: none;
}

.credential-plan-tag.el-tag :deep(.el-tag__content) {
  font-weight: 500;
}

.credential-plan-tag.is-disabled.el-tag {
  --el-tag-bg-color: #eef2f7;
  --el-tag-border-color: #d8e0ea;
  --el-tag-text-color: #64748b;
}

.credential-actions {
  display: flex;
  gap: 7px;
  justify-content: flex-end;
  min-width: max-content;
}

.credential-icon-button.el-button {
  --el-button-bg-color: #f8fafc;
  --el-button-border-color: var(--admin-border);
  --el-button-hover-bg-color: var(--brand-blue-soft);
  --el-button-hover-border-color: #cbd5e1;
  --el-button-hover-text-color: var(--brand-blue-hover);
  height: 28px;
  width: 28px;
}

.credential-icon-button.is-danger.el-button {
  --el-button-hover-bg-color: #fff1f2;
  --el-button-hover-border-color: #fecdd3;
  --el-button-hover-text-color: #e11d48;
}

.quota-grid {
  display: grid;
  gap: 10px;
}

.quota-section {
  display: grid;
  gap: 5px;
  min-width: 0;
}

.quota-row {
  align-items: end;
  display: grid;
  gap: 8px;
  grid-template-columns: minmax(0, 1fr) auto;
}

.quota-label-main {
  align-items: center;
  color: #334155;
  display: inline-flex;
  font-size: 12px;
  font-weight: 500;
  gap: 6px;
  min-width: 0;
}

.quota-label-main .el-icon {
  color: #90a0b7;
  font-size: 15px;
}

.quota-value {
  font-size: 14px;
  font-weight: 500;
}

.quota-value.is-primary {
  color: #16a34a;
}

.quota-value.is-secondary {
  color: #dc2626;
}

.quota-track {
  background: #eef3f8;
  border-radius: 999px;
  height: 6px;
  overflow: hidden;
}

.quota-track-secondary .quota-track-fill {
  background: #dc2626;
}

.quota-track-fill {
  background: #16a34a;
  border-radius: inherit;
  display: block;
  height: 100%;
  transition: width 180ms ease;
}

.quota-reset {
  color: #b8c2d1;
  font-weight: 400;
  justify-self: end;
  line-height: 1.2;
  text-align: right;
}

.credential-footer {
  align-items: center;
  border-top: 1px solid var(--admin-border-soft);
  display: grid;
  gap: 9px;
  grid-template-columns: minmax(0, 1fr) auto;
  padding-top: 9px;
}

.credential-updated-at {
  color: #94a3b8;
  font-size: 12px;
  font-weight: 500;
}

@media (max-width: 720px) {
  .credential-grid {
    grid-template-columns: 1fr;
  }

  .credential-actions {
    grid-column: auto;
    justify-content: flex-start;
  }

  .credential-footer {
    grid-template-columns: 1fr;
  }

  .quota-reset {
    justify-items: start;
    justify-self: start;
    text-align: left;
  }
}
</style>
