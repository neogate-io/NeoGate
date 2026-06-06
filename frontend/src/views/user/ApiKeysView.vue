<script setup lang="ts">
import { DocumentCopy, MoreFilled, Plus } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { ref } from 'vue'
import {
  createOwnUserKey,
  deleteOwnUserKey,
  getOwnUserKeys,
  updateOwnUserKeyStatus
} from '../../api/userKeys'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import type { UserKey } from '../../types/admin'
import { readError } from '../../utils/errors'
import { formatCompactDateTime, maskApiKey } from '../../utils/format'

const { t } = useLocale()
const createLoading = ref(false)
const deletingIds = ref(new Set<number>())
const updatingIds = ref(new Set<number>())
const createDialogVisible = ref(false)
const apiKeyName = ref('')
const newKeyDialogVisible = ref(false)
const newKey = ref('')
const { data: apiKeys, loading, reload } = useAsyncData(() => getOwnUserKeys(), [])

function formatLastActiveAt(value?: string | null) {
  return value ? formatCompactDateTime(value) : t('neverUsed')
}

async function copyText(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    ElMessage.success(t('apiKeyCopied'))
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function copyApiKey(row: UserKey) {
  await copyText(row.key)
}

async function toggleApiKeyStatus(row: UserKey, enabled: boolean) {
  updatingIds.value = new Set(updatingIds.value).add(row.id)
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
  } finally {
    const next = new Set(updatingIds.value)
    next.delete(row.id)
    updatingIds.value = next
  }
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

  createLoading.value = true
  try {
    const result = await createOwnUserKey(name)
    newKey.value = result.key
    createDialogVisible.value = false
    newKeyDialogVisible.value = true
    ElMessage.success(t('apiKeyCreated'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    createLoading.value = false
  }
}

async function confirmDeleteApiKey(row: UserKey) {
  try {
    await ElMessageBox.confirm(t('deleteApiKeyConfirm'), t('delete'), {
      confirmButtonText: t('delete'),
      cancelButtonText: t('cancel'),
      type: 'warning'
    })
  } catch {
    return
  }

  deletingIds.value = new Set(deletingIds.value).add(row.id)
  try {
    await deleteOwnUserKey(row.id)
    ElMessage.success(t('apiKeyDeleted'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    const next = new Set(deletingIds.value)
    next.delete(row.id)
    deletingIds.value = next
  }
}
</script>

<template>
  <section class="user-api-keys-view">
    <div
      v-loading="loading"
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

      <button
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
  display: grid;
  gap: 12px;
  width: min(1120px, 100%);
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
  color: #111827;
  font-size: 16px;
  font-weight: 800;
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
  color: #475569;
  display: inline-flex;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 760;
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
  color: #64748b;
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.08em;
  line-height: 1;
  text-transform: uppercase;
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
  color: #0f172a;
  flex: 1 1 auto;
  font-size: 16px;
  font-weight: 780;
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
  color: #94a3b8;
  font-size: 12px;
  font-weight: 700;
  line-height: 1.2;
}

.key-card-meta dd {
  color: #64748b;
  font-size: 13px;
  font-weight: 650;
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

.key-create-card strong {
  font-size: 15px;
  font-weight: 800;
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
  color: #111827;
  flex: 1 1 auto;
  font-family: 'SFMono-Regular', Consolas, 'Liberation Mono', monospace;
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
  .key-card-meta {
    grid-template-columns: 1fr;
  }

  .key-card-controls {
    gap: 6px;
  }
}
</style>
