<script setup lang="ts">
import { DocumentCopy, Search, Refresh, View } from '@element-plus/icons-vue'
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { adjustCredit, getUserKeys } from '../../api/userKeys'
import { getUsers } from '../../api/users'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import type { User, UserKey } from '../../types/admin'
import { readError } from '../../utils/errors'
import {
  formatCompactDateTime,
  formatDateTime,
  formatMicroUsd,
  maskApiKey,
  usdToMicroUsd
} from '../../utils/format'

const { locale, t } = useLocale()
const emailSearch = ref('')
const apiKeySearch = ref('')
const creditDialogVisible = ref(false)
const creditSaving = ref(false)
const selectedUser = ref<User | null>(null)
const amountUsd = ref(10)
const userKeysDialogVisible = ref(false)
const userKeysLoading = ref(false)
const selectedUserKeys = ref<UserKey[]>([])
const { data: users, loading, reload } = useAsyncData(loadUsers, [])

async function loadUsers() {
  return getUsers({
    email: emailSearch.value.trim(),
    apiKey: apiKeySearch.value.trim()
  })
}

function formatAvailableUsd(microUsd?: number | null) {
  return (microUsd ?? 0) === 0 ? '-' : formatMicroUsd(microUsd, 4)
}

function formatLastActiveAt(value?: string | null) {
  return value ? formatCompactDateTime(value) : t('neverUsed')
}

function openCreditDialog(row: User) {
  selectedUser.value = row
  amountUsd.value = 10
  creditDialogVisible.value = true
}

async function openUserKeysDialog(row: User) {
  selectedUser.value = row
  userKeysDialogVisible.value = true
  userKeysLoading.value = true
  selectedUserKeys.value = []
  try {
    selectedUserKeys.value = await getUserKeys({ userId: row.id })
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    userKeysLoading.value = false
  }
}

async function copyApiKey(row: UserKey) {
  try {
    await navigator.clipboard.writeText(row.key)
    ElMessage.success(t('apiKeyCopied'))
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function submitCredit() {
  if (!selectedUser.value) return
  creditSaving.value = true
  try {
    await adjustCredit('user', selectedUser.value.id, usdToMicroUsd(amountUsd.value))
    ElMessage.success(t('creditUpdated'))
    creditDialogVisible.value = false
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    creditSaving.value = false
  }
}

async function searchUsers() {
  await reload()
}

async function resetSearch() {
  emailSearch.value = ''
  apiKeySearch.value = ''
  await reload()
}
</script>

<template>
  <section class="grid user-management-view">
    <el-form class="admin-filter-bar user-filter-bar" @submit.prevent="searchUsers">
      <el-form-item class="user-search-field" :label="t('email')">
        <el-input
          v-model="emailSearch"
          clearable
          :placeholder="t('userEmailSearchPlaceholder')"
          @clear="searchUsers"
        />
      </el-form-item>
      <el-form-item class="user-search-field" :label="t('apiKey')">
        <el-input
          v-model="apiKeySearch"
          clearable
          show-password
          :placeholder="t('apiKeySearchPlaceholder')"
          @clear="searchUsers"
        />
      </el-form-item>
      <el-form-item class="user-search-actions">
        <el-button type="primary" native-type="submit" :icon="Search" :loading="loading">
          {{ t('search') }}
        </el-button>
        <el-button :icon="Refresh" @click="resetSearch">{{ t('reset') }}</el-button>
      </el-form-item>
    </el-form>

    <div class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table user-table"
        :data="users"
        stripe
      >
        <el-table-column prop="id" label="ID" width="72" />
        <el-table-column prop="email" :label="t('email')" min-width="200" />
        <el-table-column :label="t('userGroup')" min-width="120">
          <template #default="{ row }">{{ row.user_group_name }}</template>
        </el-table-column>
        <el-table-column
          :label="t('userApiKeyCount')"
          width="112"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">{{ row.user_key_count ?? '-' }}</template>
        </el-table-column>
        <el-table-column
          :label="t('availableCredit')"
          min-width="118"
          align="right"
          header-align="right"
        >
          <template #default="{ row }">{{ formatAvailableUsd(row.available_micro_usd) }}</template>
        </el-table-column>
        <el-table-column :label="t('status')" width="96" align="center" header-align="center">
          <template #default="{ row }">
            <el-tag class="static-state-tag" :type="row.status === 'enabled' ? 'success' : 'info'">
              {{ row.status === 'enabled' ? t('enabled') : t('disabled') }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="160">
          <template #default="{ row }">{{ formatDateTime(row.created_at, locale) }}</template>
        </el-table-column>
        <el-table-column :label="t('lastActiveAt')" min-width="160">
          <template #default="{ row }">{{ formatLastActiveAt(row.last_active_at) }}</template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="190" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button class="admin-action-button" :icon="View" @click="openUserKeysDialog(row)">
                {{ t('viewApiKeys') }}
              </el-button>
              <el-button class="admin-action-button" @click="openCreditDialog(row)">{{
                t('recharge')
              }}</el-button>
            </div>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog v-model="creditDialogVisible" :title="t('recharge')" width="420px">
      <el-form label-position="top">
        <el-form-item :label="t('amountUsd')">
          <el-input-number v-model="amountUsd" :min="-100000" :precision="2" :step="1" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="creditDialogVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="creditSaving" @click="submitCredit">{{
          t('save')
        }}</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="userKeysDialogVisible" :title="t('apiKeyDetails')" width="680px">
      <el-table
        v-loading="userKeysLoading"
        class="admin-table service-table user-key-detail-table"
        :data="selectedUserKeys"
        stripe
      >
        <el-table-column :label="t('name')" min-width="120">
          <template #default="{ row }">{{ row.name }}</template>
        </el-table-column>
        <el-table-column :label="t('apiKey')" min-width="240">
          <template #default="{ row }">
            <div class="user-key-cell">
              <code class="user-key-value">{{ maskApiKey(row.key) }}</code>
              <el-tooltip :content="t('copy')" placement="top">
                <el-button
                  class="user-key-copy-button"
                  :aria-label="t('copy')"
                  :icon="DocumentCopy"
                  @click="copyApiKey(row)"
                />
              </el-tooltip>
            </div>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('availableCredit')"
          width="84"
          align="right"
          header-align="right"
        >
          <template #default="{ row }">{{ formatAvailableUsd(row.available_micro_usd) }}</template>
        </el-table-column>
        <el-table-column :label="t('status')" width="80" align="center" header-align="center">
          <template #default="{ row }">
            <el-tag class="static-state-tag" :type="row.status === 'enabled' ? 'success' : 'info'">
              {{ row.status === 'enabled' ? t('enabled') : t('disabled') }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="120">
          <template #default="{ row }">
            <span class="user-key-meta-time">{{ formatCompactDateTime(row.created_at) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('lastUsed')" min-width="116">
          <template #default="{ row }">
            <span class="user-key-meta-time">{{ formatLastActiveAt(row.last_active_at) }}</span>
          </template>
        </el-table-column>
        <template #empty>
          <el-empty :description="t('noApiKeys')" />
        </template>
      </el-table>
    </el-dialog>
  </section>
</template>
