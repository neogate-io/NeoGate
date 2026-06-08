<script setup lang="ts">
import { DocumentCopy, Edit, Search, Refresh, View } from '@element-plus/icons-vue'
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { adjustCredit, getUserGroups, getUserKeys } from '../../api/userKeys'
import { getUsers, updateUser, updateUserStatus } from '../../api/users'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'
import type { User, UserGroup, UserKey } from '../../types/admin'
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
const editDialogVisible = ref(false)
const editSaving = ref(false)
const approvingUserId = ref<number | null>(null)
const selectedUser = ref<User | null>(null)
const userGroups = ref<UserGroup[]>([])
const amountUsd = ref(100)
const editForm = reactive({
  email: '',
  status: 'enabled' as User['status'],
  userGroupId: 0
})
const userKeysDialogVisible = ref(false)
const userKeysLoading = ref(false)
const selectedUserKeys = ref<UserKey[]>([])
const usersCurrentPage = ref(1)
const usersPageSize = ref(50)
const usersCursorStack = ref<(string | undefined)[]>([undefined])
const userKeysCurrentPage = ref(1)
const userKeysPageSize = ref(100)
const userKeysCursorStack = ref<(string | undefined)[]>([undefined])
const userKeysHasMore = ref(false)
const userKeysNextCursor = ref<string | null | undefined>(undefined)
const {
  data: usersPage,
  loading,
  loaded: usersLoaded,
  reload
} = useAsyncData(loadUsers, {
  items: [],
  limit: 50,
  next_cursor: null,
  has_more: false
})
const users = computed(() => usersPage.value.items)

async function loadUsers() {
  return getUsers({
    email: emailSearch.value.trim(),
    apiKey: apiKeySearch.value.trim(),
    limit: usersPageSize.value,
    cursor: usersCursorStack.value[usersCurrentPage.value - 1]
  })
}

function formatAvailableUsd(microUsd?: number | null) {
  return (microUsd ?? 0) === 0 ? '-' : formatMicroUsd(microUsd, 4)
}

function formatLastActiveAt(value?: string | null) {
  return value ? formatCompactDateTime(value) : t('neverUsed')
}

function userStatusText(status: User['status']) {
  if (status === 'enabled') return t('enabled')
  if (status === 'pending') return t('pendingApproval')
  return t('disabled')
}

function userStatusTagType(status: User['status']) {
  if (status === 'enabled') return 'success'
  if (status === 'pending') return 'warning'
  return 'info'
}

function openCreditDialog(row: User) {
  selectedUser.value = row
  amountUsd.value = 100
  creditDialogVisible.value = true
}

function openEditDialog(row: User) {
  selectedUser.value = row
  Object.assign(editForm, {
    email: row.email,
    status: row.status,
    userGroupId: row.user_group_id
  })
  editDialogVisible.value = true
}

async function openUserKeysDialog(row: User) {
  selectedUser.value = row
  userKeysDialogVisible.value = true
  userKeysLoading.value = true
  selectedUserKeys.value = []
  userKeysCurrentPage.value = 1
  userKeysCursorStack.value = [undefined]
  try {
    await loadSelectedUserKeys()
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

async function submitEditUser() {
  if (!selectedUser.value) return
  editSaving.value = true
  try {
    await updateUser(selectedUser.value.id, {
      email: editForm.email.trim(),
      status: editForm.status,
      user_group_id: editForm.userGroupId
    })
    ElMessage.success(t('userUpdated'))
    editDialogVisible.value = false
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    editSaving.value = false
  }
}

async function approveUser(row: User) {
  approvingUserId.value = row.id
  try {
    await updateUserStatus(row.id, 'enabled')
    ElMessage.success(t('userApproved'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    approvingUserId.value = null
  }
}

async function searchUsers() {
  usersCurrentPage.value = 1
  usersCursorStack.value = [undefined]
  await reload()
}

async function resetSearch() {
  emailSearch.value = ''
  apiKeySearch.value = ''
  usersCurrentPage.value = 1
  usersCursorStack.value = [undefined]
  await reload()
}

async function nextUsersPage() {
  if (!usersPage.value.has_more || !usersPage.value.next_cursor) return
  usersCursorStack.value[usersCurrentPage.value] = usersPage.value.next_cursor
  usersCurrentPage.value += 1
  await reload()
}

async function previousUsersPage() {
  if (usersCurrentPage.value <= 1) return
  usersCurrentPage.value -= 1
  await reload()
}

async function handleUsersPageSizeChange(size: number) {
  usersPageSize.value = size
  usersCurrentPage.value = 1
  usersCursorStack.value = [undefined]
  await reload()
}

async function loadSelectedUserKeys() {
  if (!selectedUser.value) return
  const page = await getUserKeys({
    userId: selectedUser.value.id,
    limit: userKeysPageSize.value,
    cursor: userKeysCursorStack.value[userKeysCurrentPage.value - 1]
  })
  selectedUserKeys.value = page.items
  userKeysHasMore.value = Boolean(page.has_more)
  userKeysNextCursor.value = page.next_cursor
}

async function nextUserKeysPage() {
  if (!userKeysHasMore.value || !userKeysNextCursor.value) return
  userKeysCursorStack.value[userKeysCurrentPage.value] = userKeysNextCursor.value
  userKeysCurrentPage.value += 1
  userKeysLoading.value = true
  try {
    await loadSelectedUserKeys()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    userKeysLoading.value = false
  }
}

async function previousUserKeysPage() {
  if (userKeysCurrentPage.value <= 1) return
  userKeysCurrentPage.value -= 1
  userKeysLoading.value = true
  try {
    await loadSelectedUserKeys()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    userKeysLoading.value = false
  }
}

async function loadUserGroups() {
  try {
    userGroups.value = await getUserGroups()
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

onMounted(loadUserGroups)
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

    <div v-if="!usersLoaded" v-loading="true" class="service-table-panel user-table-loading">
      <div class="user-table-loading-head">
        <span></span>
        <span></span>
        <span></span>
        <span></span>
        <span></span>
      </div>
      <div class="user-table-loading-row"></div>
      <div class="user-table-loading-row"></div>
      <div class="user-table-loading-row"></div>
    </div>

    <div v-else class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table user-table"
        :data="users"
        stripe
      >
        <el-table-column prop="id" label="ID" width="72" />
        <el-table-column prop="email" :label="t('email')" min-width="200" />
        <el-table-column :label="t('userGroup')" width="96">
          <template #default="{ row }">{{ row.user_group_name }}</template>
        </el-table-column>
        <el-table-column
          :label="t('availableCredit')"
          width="104"
          align="right"
          header-align="right"
        >
          <template #default="{ row }">{{ formatAvailableUsd(row.available_micro_usd) }}</template>
        </el-table-column>
        <el-table-column :label="t('status')" width="96" align="center" header-align="center">
          <template #default="{ row }">
            <el-tag class="static-state-tag" :type="userStatusTagType(row.status)">
              {{ userStatusText(row.status) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="160">
          <template #default="{ row }">{{ formatDateTime(row.created_at, locale) }}</template>
        </el-table-column>
        <el-table-column :label="t('lastActiveAt')" min-width="160">
          <template #default="{ row }">{{ formatLastActiveAt(row.last_active_at) }}</template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="300" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button
                v-if="row.status === 'pending'"
                class="admin-action-button"
                type="primary"
                :loading="approvingUserId === row.id"
                @click="approveUser(row)"
              >
                {{ t('approve') }}
              </el-button>
              <el-button class="admin-action-button" :icon="View" @click="openUserKeysDialog(row)">
                {{ t('viewApiKeys') }}
              </el-button>
              <el-button class="admin-action-button" :icon="Edit" @click="openEditDialog(row)">
                {{ t('edit') }}
              </el-button>
              <el-button class="admin-action-button" @click="openCreditDialog(row)">{{
                t('recharge')
              }}</el-button>
            </div>
          </template>
        </el-table-column>
        <template #empty>
          <el-empty :description="t('noData')" />
        </template>
      </el-table>
    </div>

    <div v-if="usersLoaded" class="admin-pagination-bar">
      <div class="admin-pagination-summary">
        <span class="admin-result-count">
          {{ t('currentPageItems') }} {{ users.length.toLocaleString(locale) }}
          {{ t('itemsUnit') }}
        </span>
      </div>
      <div class="admin-pagination-controls">
        <div class="admin-page-size-control">
          <span class="admin-page-label">{{ t('pageSize') }}</span>
          <el-select
            v-model="usersPageSize"
            class="admin-page-size"
            @change="handleUsersPageSizeChange"
          >
            <el-option :value="20" label="20" />
            <el-option :value="50" label="50" />
            <el-option :value="100" label="100" />
          </el-select>
        </div>
        <span class="admin-result-count">{{ t('currentPage') }} {{ usersCurrentPage }}</span>
        <div class="admin-page-buttons">
          <el-button :disabled="usersCurrentPage <= 1 || loading" @click="previousUsersPage">
            {{ t('previousPage') }}
          </el-button>
          <el-button :disabled="!usersPage.has_more || loading" @click="nextUsersPage">
            {{ t('nextPage') }}
          </el-button>
        </div>
      </div>
    </div>

    <el-dialog
      v-model="editDialogVisible"
      class="user-admin-dialog user-edit-dialog"
      :title="t('editUser')"
      width="520px"
    >
      <div class="user-dialog-body">
        <el-form class="user-dialog-form" label-position="top" @submit.prevent="submitEditUser">
          <el-form-item class="user-dialog-field is-wide" :label="t('email')">
            <el-input v-model="editForm.email" />
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('status')">
            <el-select v-model="editForm.status" class="user-edit-select">
              <el-option :label="t('enabled')" value="enabled" />
              <el-option :label="t('disabled')" value="disabled" />
              <el-option :label="t('pendingApproval')" value="pending" />
            </el-select>
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('userGroup')">
            <el-select v-model="editForm.userGroupId" class="user-edit-select">
              <el-option
                v-for="group in userGroups"
                :key="group.id"
                :label="`${group.name} (${group.code})`"
                :value="group.id"
                :disabled="!group.enabled"
              />
            </el-select>
          </el-form-item>
          <button class="hidden-submit" type="submit" />
        </el-form>
      </div>
      <template #footer>
        <div class="admin-dialog-footer user-dialog-footer">
          <el-button @click="editDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="editSaving" @click="submitEditUser">{{
            t('save')
          }}</el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="creditDialogVisible"
      class="user-admin-dialog user-credit-dialog"
      :title="t('recharge')"
      width="460px"
    >
      <div class="user-dialog-body">
        <el-form class="user-dialog-form is-single" label-position="top">
          <el-form-item class="user-dialog-field user-credit-amount-field" :label="t('amountUsd')">
            <el-input-number v-model="amountUsd" :min="-100000" :precision="2" :step="1" />
          </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <div class="admin-dialog-footer user-dialog-footer">
          <el-button @click="creditDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="creditSaving" @click="submitCredit">{{
            t('save')
          }}</el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="userKeysDialogVisible"
      class="user-admin-dialog user-keys-dialog"
      :title="t('apiKeyDetails')"
      width="860px"
    >
      <div class="user-dialog-body user-keys-dialog-body">
        <div class="service-table-panel user-key-detail-panel">
          <el-table
            v-loading="userKeysLoading"
            class="admin-table service-table user-key-detail-table"
            :data="selectedUserKeys"
            stripe
          >
            <el-table-column :label="t('name')" min-width="112">
              <template #default="{ row }">{{ row.name }}</template>
            </el-table-column>
            <el-table-column :label="t('apiKey')" min-width="220">
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
              width="96"
              align="right"
              header-align="right"
            >
              <template #default="{ row }">{{
                formatAvailableUsd(row.available_micro_usd)
              }}</template>
            </el-table-column>
            <el-table-column :label="t('status')" width="84" align="center" header-align="center">
              <template #default="{ row }">
                <el-tag
                  class="static-state-tag"
                  :type="row.status === 'enabled' ? 'success' : 'info'"
                >
                  {{ row.status === 'enabled' ? t('enabled') : t('disabled') }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column :label="t('createdAt')" min-width="116">
              <template #default="{ row }">
                <span class="user-key-meta-time">{{ formatCompactDateTime(row.created_at) }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('lastUsed')" min-width="108">
              <template #default="{ row }">
                <span class="user-key-meta-time">{{ formatLastActiveAt(row.last_active_at) }}</span>
              </template>
            </el-table-column>
            <template #empty>
              <el-empty :description="t('noApiKeys')" />
            </template>
          </el-table>
        </div>

        <div class="admin-pagination-bar user-key-pagination">
          <div class="admin-pagination-summary">
            <span class="admin-result-count">
              {{ t('currentPageItems') }} {{ selectedUserKeys.length.toLocaleString(locale) }}
              {{ t('itemsUnit') }}
            </span>
          </div>
          <div class="admin-pagination-controls">
            <span class="admin-result-count">{{ t('currentPage') }} {{ userKeysCurrentPage }}</span>
            <div class="admin-page-buttons">
              <el-button
                :disabled="userKeysCurrentPage <= 1 || userKeysLoading"
                @click="previousUserKeysPage"
              >
                {{ t('previousPage') }}
              </el-button>
              <el-button :disabled="!userKeysHasMore || userKeysLoading" @click="nextUserKeysPage">
                {{ t('nextPage') }}
              </el-button>
            </div>
          </div>
        </div>
      </div>
    </el-dialog>
  </section>
</template>

<style scoped>
.user-table-loading {
  min-height: 236px;
  overflow: hidden;
}

.user-table-loading-head {
  align-items: center;
  background: #f9fbfd;
  border-bottom: 1px solid #dfe8f2;
  display: grid;
  gap: 28px;
  grid-template-columns: 54px minmax(180px, 1fr) 86px 104px 96px;
  height: 48px;
  min-width: 980px;
  padding: 0 300px 0 14px;
}

.user-table-loading-head span,
.user-table-loading-row::before,
.user-table-loading-row::after,
.user-table-loading-row span {
  background: #e8eef6;
  border-radius: 999px;
  content: '';
  display: block;
  height: 12px;
}

.user-table-loading-head span:nth-child(1) {
  width: 20px;
}

.user-table-loading-head span:nth-child(2) {
  width: 48px;
}

.user-table-loading-head span:nth-child(3) {
  width: 52px;
}

.user-table-loading-head span:nth-child(4) {
  width: 72px;
}

.user-table-loading-head span:nth-child(5) {
  width: 48px;
}

.user-table-loading-row {
  align-items: center;
  border-bottom: 1px solid #edf3f8;
  display: grid;
  gap: 28px;
  grid-template-columns: 54px minmax(180px, 1fr) 86px 104px 96px;
  height: 62px;
  min-width: 980px;
  padding: 0 300px 0 14px;
}

.user-table-loading-row::before {
  width: 28px;
}

.user-table-loading-row::after {
  width: min(240px, 100%);
}

.user-table-loading-row span {
  width: 64px;
}

.user-dialog-body {
  display: grid;
  gap: 16px;
}

.user-dialog-form {
  display: grid;
  gap: 16px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.user-dialog-form.is-single {
  grid-template-columns: 1fr;
}

.user-dialog-field {
  margin-bottom: 0;
  min-width: 0;
}

.user-dialog-field.is-wide {
  grid-column: 1 / -1;
}

.user-dialog-field :deep(.el-form-item__label) {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 720;
  line-height: 1.2;
  margin-bottom: 8px;
  padding: 0;
}

.user-dialog-field :deep(.el-input),
.user-dialog-field :deep(.el-input-number),
.user-dialog-field :deep(.el-select) {
  width: 100%;
}

.user-dialog-field :deep(.el-input__wrapper),
.user-dialog-field :deep(.el-input-number),
.user-dialog-field :deep(.el-select__wrapper) {
  border-radius: 7px;
  min-height: 36px;
}

.user-credit-amount-field :deep(.el-input-number) {
  width: 180px;
}

.user-dialog-footer {
  margin: 0;
}

.user-keys-dialog-body {
  gap: 12px;
}

.user-key-detail-panel {
  max-height: min(58dvh, 520px);
}

.user-key-detail-table {
  min-width: 100%;
}

.user-key-pagination {
  padding-top: 2px;
}

.user-edit-select {
  width: 100%;
}

.hidden-submit {
  display: none;
}

:global(.user-admin-dialog) {
  border-radius: 8px;
  overflow: hidden;
}

:global(.user-admin-dialog .el-dialog__header) {
  border-bottom: 1px solid var(--admin-border-soft);
  margin: 0;
  padding: 18px 20px 16px;
}

:global(.user-admin-dialog .el-dialog__title) {
  color: var(--admin-text);
  font-size: 17px;
  font-weight: 760;
  line-height: 1.25;
}

:global(.user-admin-dialog .el-dialog__headerbtn) {
  height: 52px;
  top: 0;
  width: 52px;
}

:global(.user-admin-dialog .el-dialog__body) {
  padding: 18px 20px;
}

:global(.user-admin-dialog .el-dialog__footer) {
  border-top: 1px solid var(--admin-border-soft);
  padding: 14px 20px 16px;
}

:global(.user-admin-dialog .el-dialog__footer .admin-dialog-footer) {
  border-top: 0;
  padding-top: 0;
}

:global(.user-admin-dialog .el-button) {
  border-radius: 7px;
}

:global(.user-credit-dialog .el-dialog__body) {
  padding-bottom: 20px;
}

:global(.user-keys-dialog .el-dialog__body) {
  padding: 16px 18px 18px;
}

:global(.user-keys-dialog .el-dialog__footer) {
  display: none;
}

@media (max-width: 640px) {
  .user-dialog-form {
    grid-template-columns: 1fr;
  }

  .user-dialog-footer {
    justify-content: stretch;
  }

  .user-dialog-footer .el-button {
    flex: 1 1 0;
    min-width: 0;
  }

  .user-credit-amount-field :deep(.el-input-number) {
    width: 100%;
  }

  .user-key-detail-panel {
    max-height: 54dvh;
  }

  .user-key-pagination {
    align-items: stretch;
  }

  .user-key-pagination .admin-pagination-controls,
  .user-key-pagination .admin-page-buttons {
    justify-content: stretch;
    width: 100%;
  }

  .user-key-pagination .admin-page-buttons .el-button {
    flex: 1 1 0;
    min-width: 0;
  }

  :global(.user-admin-dialog .el-dialog__header) {
    padding: 16px 18px 14px;
  }

  :global(.user-admin-dialog .el-dialog__body) {
    padding: 16px 18px;
  }

  :global(.user-admin-dialog .el-dialog__footer) {
    padding: 14px 18px 16px;
  }
}
</style>
