<script setup lang="ts">
import { DocumentCopy, MoreFilled, Plus } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { computed, inject, ref, type Ref } from 'vue'
import {
  createOwnUserKey,
  deleteOwnUserKey,
  getOwnUserKeys,
  updateOwnUserKeyStatus
} from '../../api/userKeys'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type { UserKey } from '../../types/admin'
import type { ServicePolicy } from '../../api/policy'
import { copyTextWithMessage } from '../../utils/clipboard'
import { createConfirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import { formatCompactDateTime, maskApiKey } from '../../utils/format'

const { t } = useLocale()
const confirmDialog = createConfirmAction(() => t('cancel'))
const createLoading = ref(false)
const deletingIds = useReactiveSet<number>()
const updatingIds = useReactiveSet<number>()
const createDialogVisible = ref(false)
const apiKeyName = ref('')
const newKeyDialogVisible = ref(false)
const newKey = ref('')
const apiBaseUrl = computed(() => `${window.location.origin}/v1`)
const keySkeletonCount = 3
const servicePolicy = inject<Ref<ServicePolicy | null>>('servicePolicy')!
const canCreateDefaultApiKey = computed(() => servicePolicy.value?.service_mode === 'paid')
const {
  data: apiKeys,
  loading,
  loaded: keysLoaded,
  reload
} = useAsyncData(() => getOwnUserKeys(), [])
const showApiKeyEmptyState = computed(
  () => keysLoaded.value && apiKeys.value.length === 0 && !canCreateDefaultApiKey.value
)

function formatLastActiveAt(value?: string | null) {
  return value ? formatCompactDateTime(value) : t('neverUsed')
}

async function copyText(value: string, successMessage = t('apiKeyCopied')) {
  await copyTextWithMessage(value, successMessage)
}

async function copyApiKey(row: UserKey) {
  await copyText(row.key)
}

async function copyBaseUrl() {
  await copyText(apiBaseUrl.value, t('baseUrlCopied'))
}

async function toggleApiKeyStatus(row: UserKey, enabled: boolean) {
  await updatingIds.withItem(row.id, async () => {
    try {
      const nextStatus = enabled ? 'enabled' : 'disabled'
      const updated = await updateOwnUserKeyStatus(row.id, nextStatus)
      const index = apiKeys.value.findIndex((key) => key.id === row.id)
      if (index >= 0) {
        apiKeys.value.splice(index, 1, updated)
      }
      ElMessage.success(nextStatus === 'enabled' ? t('apiKeyEnabled') : t('apiKeyDisabled'))
    } catch (err) {
      ElMessage.error(readError(err))
      await reload()
    }
  })
}

function handleKeyMenuCommand(row: UserKey, command: string | number) {
  if (command === 'toggle') {
    void toggleApiKeyStatus(row, row.status !== 'enabled')
    return
  }

  if (command === 'delete') {
    void confirmDeleteApiKey(row)
  }
}

function openCreateDialog() {
  apiKeyName.value = ''
  createDialogVisible.value = true
}

async function createApiKey() {
  const name = apiKeyName.value.trim()
  if (!name) {
    ElMessage.error(t('apiKeyNameRequired'))
    return
  }

  await withLoading(createLoading, async () => {
    try {
      const result = await createOwnUserKey(name)
      newKey.value = result.key
      createDialogVisible.value = false
      newKeyDialogVisible.value = true
      ElMessage.success(t('apiKeyCreated'))
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function confirmDeleteApiKey(row: UserKey) {
  const confirmed = await confirmDialog(t('deleteApiKeyConfirm'), t('delete'), {
    confirmText: t('delete'),
    danger: true,
    type: 'warning'
  })
  if (!confirmed) return

  await deletingIds.withItem(row.id, async () => {
    try {
      await deleteOwnUserKey(row.id)
      ElMessage.success(t('apiKeyDeleted'))
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}
</script>

<template>
  <section class="user-api-keys-view">
    <section class="api-base-url-panel" :aria-label="t('baseUrl')">
      <span>{{ t('baseUrl') }}:</span>
      <div class="api-base-url-row">
        <code>{{ apiBaseUrl }}</code>
        <el-tooltip :content="t('copy')" placement="top">
          <el-button
            class="key-inline-copy"
            :aria-label="t('copy')"
            :icon="DocumentCopy"
            @click="copyBaseUrl"
          />
        </el-tooltip>
      </div>
    </section>

    <div v-if="!keysLoaded" class="key-card-grid key-loading-grid" aria-hidden="true">
      <article v-for="index in keySkeletonCount" :key="index" class="user-panel key-card-skeleton">
        <span></span>
        <span></span>
        <span></span>
      </article>
    </div>
    <div
      v-else
      v-loading="loading && apiKeys.length > 0"
      class="key-card-grid"
      :class="{ 'is-empty': apiKeys.length === 0 }"
      role="list"
    >
      <article v-for="row in apiKeys" :key="row.id" class="user-panel key-card" role="listitem">
        <div class="key-list-name-row">
          <strong>{{ row.name }}</strong>
          <div class="key-card-controls">
            <span class="key-status-pill" :class="{ 'is-enabled': row.status === 'enabled' }">
              {{ row.status === 'enabled' ? t('enabled') : t('disabled') }}
            </span>
            <el-dropdown
              trigger="click"
              placement="bottom-end"
              @command="(command: string | number) => handleKeyMenuCommand(row, command)"
            >
              <el-button
                class="key-more-button"
                :aria-label="t('moreActions')"
                :icon="MoreFilled"
                :loading="updatingIds.has(row.id) || deletingIds.has(row.id)"
              />
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="toggle">
                    {{ row.status === 'enabled' ? t('pauseApiKey') : t('enableApiKey') }}
                  </el-dropdown-item>
                  <el-dropdown-item command="delete" divided class="danger-dropdown-item">
                    {{ t('delete') }}
                  </el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </div>
        </div>

        <div class="key-list-secret">
          <span>{{ t('secretKey') }}</span>
          <div class="key-secret-value-row">
            <code class="user-key-value">{{ maskApiKey(row.key) }}</code>
            <el-tooltip :content="t('copy')" placement="top">
              <el-button
                class="key-inline-copy"
                :aria-label="t('copy')"
                :icon="DocumentCopy"
                @click="copyApiKey(row)"
              />
            </el-tooltip>
          </div>
        </div>

        <dl class="key-card-meta">
          <div>
            <dt>{{ t('createdAt') }}</dt>
            <dd>{{ formatCompactDateTime(row.created_at) }}</dd>
          </div>
          <div>
            <dt>{{ t('lastActiveAt') }}</dt>
            <dd>{{ formatLastActiveAt(row.last_active_at) }}</dd>
          </div>
        </dl>
      </article>

      <article v-if="showApiKeyEmptyState" class="user-panel key-empty-state" role="listitem">
        <el-empty :description="t('noApiKeys')">
          <p class="key-empty-hint">{{ t('apiKeysInternalEmptyHint') }}</p>
        </el-empty>
      </article>

      <button
        v-if="canCreateDefaultApiKey"
        class="user-panel key-create-card"
        type="button"
        role="listitem"
        @click="openCreateDialog"
      >
        <span class="key-create-icon"
          ><el-icon><Plus /></el-icon
        ></span>
        <strong>{{ t('createNewApiKey') }}</strong>
      </button>
    </div>

    <el-dialog v-model="createDialogVisible" :title="t('createApiKey')" width="420px">
      <el-form class="create-api-key-form" @submit.prevent="createApiKey">
        <el-form-item :label="t('apiKeyName')">
          <el-input
            v-model="apiKeyName"
            maxlength="80"
            show-word-limit
            :placeholder="t('apiKeyNamePlaceholder')"
            @keyup.enter="createApiKey"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button :disabled="createLoading" @click="createDialogVisible = false">{{
          t('cancel')
        }}</el-button>
        <el-button type="primary" :loading="createLoading" @click="createApiKey">{{
          t('create')
        }}</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="newKeyDialogVisible" :title="t('newApiKey')" width="520px">
      <p class="new-api-key-hint">{{ t('newApiKeyHint') }}</p>
      <p class="new-api-key-warning">{{ t('oneTimeApiKeyHint') }}</p>
      <div class="new-api-key-box">
        <code>{{ newKey }}</code>
        <el-tooltip :content="t('copy')" placement="top">
          <el-button
            class="user-key-copy-button"
            :aria-label="t('copy')"
            :icon="DocumentCopy"
            @click="copyText(newKey)"
          />
        </el-tooltip>
      </div>
      <template #footer>
        <el-button type="primary" @click="newKeyDialogVisible = false">{{ t('save') }}</el-button>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.user-api-keys-view {
  color: #354154;
  display: grid;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC',
    'Microsoft YaHei', sans-serif;
  gap: 12px;
  width: min(1120px, 100%);
}

.api-base-url-panel {
  align-items: center;
  display: flex;
  gap: 14px;
  justify-content: flex-start;
  min-width: 0;
  padding: 0 2px 2px;
}

.api-base-url-panel > span {
  color: #6f7b8f;
  flex: 0 0 auto;
  font-size: 12.5px;
  font-weight: 400;
  letter-spacing: 0;
  line-height: 1;
}

.api-base-url-row {
  align-items: center;
  display: flex;
  flex: 0 1 auto;
  gap: 8px;
  justify-content: flex-start;
  min-width: 0;
}

.api-base-url-row code {
  color: #354154;
  font-family: inherit;
  font-size: 13px;
  font-weight: 400;
  letter-spacing: 0;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.key-card-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 340px));
  justify-content: start;
  min-height: 160px;
}

.key-card,
.key-create-card {
  display: grid;
  gap: 14px;
  min-height: 132px;
  padding: 16px;
  transition:
    border-color 0.16s ease,
    box-shadow 0.16s ease;
}

.key-card-skeleton {
  display: grid;
  gap: 14px;
  min-height: 132px;
  padding: 16px;
}

.key-card-skeleton span {
  background: var(--skeleton-gradient);
  background-size: 220% 100%;
  border-radius: 999px;
  display: block;
  height: 12px;
}

.key-card-skeleton span:nth-child(1) {
  width: 46%;
}

.key-card-skeleton span:nth-child(2) {
  border-radius: 8px;
  height: 46px;
  width: 100%;
}

.key-card-skeleton span:nth-child(3) {
  width: 72%;
}

.key-card:hover,
.key-create-card:hover {
  border-color: var(--user-primary-border, #b7dcf2);
  box-shadow:
    0 1px 2px rgba(15, 23, 42, 0.03),
    0 14px 34px rgba(15, 23, 42, 0.055);
}

.key-list-name-row {
  align-items: center;
  display: flex;
  justify-content: space-between;
  gap: 10px;
  min-width: 0;
}

.key-list-name-row strong {
  color: #1f2937;
  font-size: 15px;
  font-weight: 500;
  line-height: 1.2;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.key-card-controls {
  align-items: center;
  display: inline-flex;
  flex: 0 0 auto;
  gap: 8px;
}

.key-status-pill {
  align-items: center;
  background: #f1f5f9;
  border: 1px solid #e2e8f0;
  border-radius: 999px;
  color: #586579;
  display: inline-flex;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 400;
  gap: 6px;
  line-height: 1;
  padding: 6px 9px;
}

.key-status-pill::before {
  background: #94a3b8;
  border-radius: 999px;
  content: '';
  display: block;
  height: 8px;
  width: 8px;
}

.key-status-pill.is-enabled {
  background: #ecfdf3;
  border-color: #bbf7d0;
  color: #166534;
}

.key-status-pill.is-enabled::before {
  background: #22c55e;
  box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.13);
}

.key-list-secret {
  background: #f8fafc;
  border: 1px solid #d8e0ea;
  border-radius: 10px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.9);
  display: grid;
  gap: 8px;
  min-width: 0;
  padding: 13px 14px;
  width: 100%;
}

.key-list-secret span {
  color: #7b8798;
  font-size: 12px;
  font-weight: 400;
  letter-spacing: 0;
  line-height: 1;
}

.key-list-secret div {
  min-width: 0;
}

.key-secret-value-row {
  align-items: center;
  display: flex;
  gap: 8px;
}

.key-list-secret .user-key-value {
  color: #354154;
  flex: 1 1 auto;
  font-family: inherit;
  font-size: 13px;
  font-weight: 400;
  letter-spacing: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.key-inline-copy.el-button {
  background: transparent;
  border-color: transparent;
  color: #64748b;
  flex: 0 0 auto;
  height: 28px;
  min-width: 28px;
  padding: 0;
  width: 28px;
}

.key-inline-copy.el-button:hover,
.key-inline-copy.el-button:focus {
  background: #eef4fb;
  border-color: #d8e0ea;
  color: var(--user-primary, #168bd3);
}

.key-card-meta {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin: 0;
}

.key-card-meta div {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.key-card-meta dt {
  color: #929daf;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.2;
}

.key-card-meta dd {
  color: #586579;
  font-size: 13px;
  font-weight: 400;
  line-height: 1.25;
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.key-more-button.el-button {
  background: transparent;
  border-color: transparent;
  border-radius: 8px;
  color: #64748b;
  height: 30px;
  min-width: 34px;
  padding: 0;
  width: 30px;
}

.key-more-button.el-button:hover,
.key-more-button.el-button:focus {
  background: #f1f5f9;
  border-color: #e2e8f0;
  color: #0f172a;
}

.key-create-card {
  align-content: center;
  border-style: dashed;
  color: var(--user-primary, #168bd3);
  cursor: pointer;
  justify-items: center;
  text-align: center;
}

.key-card-grid.is-empty .key-create-card {
  min-height: 132px;
}

.key-empty-state {
  align-items: center;
  display: flex;
  grid-column: 1 / -1;
  justify-content: center;
  min-height: 188px;
  padding: 24px;
}

.key-empty-state :deep(.el-empty) {
  padding: 0;
}

.key-empty-hint {
  color: #6f7b8f;
  font-size: 13px;
  line-height: 1.6;
  margin: 0 auto;
  max-width: 420px;
  text-align: center;
}

.key-create-card strong {
  font-size: 14px;
  font-weight: 500;
}

.key-create-icon {
  align-items: center;
  background: var(--user-primary-soft, #eef4f9);
  border: 1px solid var(--user-primary-border, #b7dcf2);
  border-radius: 999px;
  color: var(--user-primary, #168bd3);
  display: inline-flex;
  height: 40px;
  justify-content: center;
  width: 40px;
}

:global(.danger-dropdown-item) {
  color: #dc2626;
}

.new-api-key-hint {
  color: var(--el-text-color-secondary);
  font-size: 13px;
  line-height: 1.6;
  margin: 0 0 12px;
}

.new-api-key-warning {
  background: #f5f8ff;
  border: 1px solid var(--user-primary-border, #b7dcf2);
  border-radius: 8px;
  color: var(--user-primary-hover, #0f76b8);
  font-size: 13px;
  line-height: 1.5;
  margin: 0 0 12px;
  padding: 10px 12px;
}

.create-api-key-form {
  padding-top: 4px;
}

.new-api-key-box {
  align-items: center;
  background: #f7f9fc;
  border: 1px solid #d9e2ef;
  border-radius: 8px;
  display: flex;
  gap: 10px;
  padding: 12px;
}

.new-api-key-box code {
  color: #354154;
  flex: 1 1 auto;
  font-family: inherit;
  font-size: 13px;
  line-height: 1.5;
  min-width: 0;
  overflow-wrap: anywhere;
}

@media (max-width: 900px) {
  .key-card-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 560px) {
  .api-base-url-panel {
    align-items: start;
    display: grid;
    gap: 8px;
  }

  .api-base-url-row {
    justify-content: start;
  }

  .key-card-meta {
    grid-template-columns: 1fr;
  }

  .key-card-controls {
    gap: 6px;
  }
}
</style>
