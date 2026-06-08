<script setup lang="ts">
import {
  CircleCheckFilled,
  Delete,
  DocumentCopy,
  Download,
  Edit,
  Key,
  Money,
  Plus,
  Refresh,
  Search,
  UserFilled,
  WarningFilled
} from '@element-plus/icons-vue'
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { adjustCredit, getUserGroups, getUserKeys } from '../../api/userKeys'
import { createUser, deleteUser, getUsers, updateUser, updateUserStatus } from '../../api/users'
import { getAdminServicePolicy, type ServicePolicy } from '../../api/policy'
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

defineOptions({
  name: 'UsersView'
})

type CreditLike = Pick<
  User | UserKey,
  'balance_micro_usd' | 'reserved_micro_usd' | 'available_micro_usd'
>

const emailSearch = ref('')
const apiKeySearch = ref('')
const creditDialogVisible = ref(false)
const creditSaving = ref(false)
const createDialogVisible = ref(false)
const createSaving = ref(false)
const editDialogVisible = ref(false)
const editSaving = ref(false)
const deletingUserId = ref<number | null>(null)
const approvingUserId = ref<number | null>(null)
const selectedUser = ref<User | null>(null)
const userGroups = ref<UserGroup[]>([])
const servicePolicy = ref<ServicePolicy | null>(null)
const amountUsd = ref(100)
const jumpTargetPage = ref(1)
const createForm = reactive({
  email: '',
  status: 'enabled' as User['status'],
  userGroupId: 0
})
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
const usersInitialLoading = computed(() => !usersLoaded.value)
const hasUserPagination = computed(
  () => usersCurrentPage.value > 1 || Boolean(usersPage.value.has_more)
)
const rechargePreviewMicroUsd = computed(() => {
  if (!selectedUser.value) return usdToMicroUsd(amountUsd.value)
  return selectedUser.value.balance_micro_usd + usdToMicroUsd(amountUsd.value)
})

async function loadUsers() {
  return getUsers({
    email: emailSearch.value.trim(),
    apiKey: apiKeySearch.value.trim(),
    limit: usersPageSize.value,
    cursor: usersCursorStack.value[usersCurrentPage.value - 1]
  })
}

function resetUsersPagination(page = 1) {
  usersCurrentPage.value = page
  usersCursorStack.value = [undefined]
  jumpTargetPage.value = page
}

function creditRequired() {
  return servicePolicy.value?.credit_required ?? true
}

function formatAvailableUsd(row: Pick<CreditLike, 'available_micro_usd'>) {
  if (!creditRequired()) return t('unlimitedCredit')
  if (row.available_micro_usd <= 0) return t('creditDepleted')
  return formatMicroUsd(row.available_micro_usd, 2)
}

function creditCellClass(row: Pick<CreditLike, 'available_micro_usd'>) {
  if (!creditRequired()) return 'is-unlimited'
  return row.available_micro_usd <= 0 ? 'is-depleted' : 'is-available'
}

function creditTooltip(row: CreditLike) {
  if (!creditRequired()) return t('creditUnlimitedTooltip')
  return [
    `${t('totalCredit')}: ${formatMicroUsd(row.balance_micro_usd, 2)}`,
    `${t('reservedCredit')}: ${formatMicroUsd(row.reserved_micro_usd, 2)}`,
    `${t('remainingCredit')}: ${formatMicroUsd(row.available_micro_usd, 2)}`
  ].join('\n')
}

function formatLastActiveAt(value?: string | null) {
  return value ? `${formatCompactDateTime(value)} · ${formatRelativeTime(value)}` : t('neverActive')
}

function formatRelativeTime(value?: string | null) {
  if (!value) return t('neverActive')
  const timestamp = new Date(value).getTime()
  if (Number.isNaN(timestamp)) return '-'
  const diffSeconds = Math.round((timestamp - Date.now()) / 1000)
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['year', 60 * 60 * 24 * 365],
    ['month', 60 * 60 * 24 * 30],
    ['day', 60 * 60 * 24],
    ['hour', 60 * 60],
    ['minute', 60]
  ]
  const formatter = new Intl.RelativeTimeFormat(locale.value, { numeric: 'auto' })
  for (const [unit, seconds] of units) {
    if (Math.abs(diffSeconds) >= seconds) {
      return formatter.format(Math.round(diffSeconds / seconds), unit)
    }
  }
  return formatter.format(diffSeconds, 'second')
}

function userStatusText(status: User['status']) {
  if (status === 'enabled') return t('enabled')
  if (status === 'pending') return t('pendingApproval')
  return t('disabled')
}

function userStatusIcon(status: User['status']) {
  if (status === 'enabled') return CircleCheckFilled
  if (status === 'pending') return WarningFilled
  return WarningFilled
}

function userStatusTone(status: User['status']) {
  if (status === 'enabled') return 'success'
  if (status === 'pending') return 'warning'
  return 'neutral'
}

function userGroupTone(row: User) {
  if (row.user_group_code === 'default') return 'default'
  if (/pro|premium|vip|advanced|高级/i.test(`${row.user_group_code} ${row.user_group_name}`)) {
    return 'premium'
  }
  return 'standard'
}

function openCreateDialog() {
  Object.assign(createForm, {
    email: '',
    status: 'enabled',
    userGroupId:
      userGroups.value.find((group) => group.is_default)?.id ?? userGroups.value[0]?.id ?? 0
  })
  createDialogVisible.value = true
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

async function confirmDialog(
  message: string,
  title: string,
  confirmButtonText: string,
  type: 'info' | 'warning'
) {
  try {
    await ElMessageBox.confirm(message, title, {
      confirmButtonText,
      cancelButtonText: t('cancel'),
      type
    })
    return true
  } catch {
    return false
  }
}

function userStatusConfirmMessage(email: string, status: User['status']) {
  return t('changeUserStatusConfirm')
    .replace('{email}', email)
    .replace('{status}', userStatusText(status))
}

function userStatusConfirmType(status: User['status']) {
  return status === 'disabled' ? 'warning' : 'info'
}

function confirmStatusChange(email: string, status: User['status']) {
  return confirmDialog(
    userStatusConfirmMessage(email, status),
    t('confirmAction'),
    t('save'),
    userStatusConfirmType(status)
  )
}

async function submitCreateUser() {
  createSaving.value = true
  try {
    const created = await createUser({
      email: createForm.email.trim(),
      status: createForm.status
    })
    if (createForm.userGroupId && createForm.userGroupId !== created.user_group_id) {
      await updateUser(created.id, { user_group_id: createForm.userGroupId })
    }
    ElMessage.success(t('userCreated'))
    createDialogVisible.value = false
    await searchUsers()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    createSaving.value = false
  }
}

async function openUserKeysDialog(row: User) {
  selectedUser.value = row
  userKeysDialogVisible.value = true
  selectedUserKeys.value = []
  userKeysCurrentPage.value = 1
  userKeysCursorStack.value = [undefined]
  await withUserKeysLoading(loadSelectedUserKeys)
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
  if (selectedUser.value.status !== editForm.status) {
    const confirmed = await confirmStatusChange(selectedUser.value.email, editForm.status)
    if (!confirmed) return
  }
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
  await confirmUserStatusChange(row, 'enabled')
}

async function confirmUserStatusChange(row: User, status: User['status']) {
  if (row.status === status) return
  const confirmed = await confirmStatusChange(row.email, status)
  if (!confirmed) return
  approvingUserId.value = row.id
  try {
    await updateUserStatus(row.id, status)
    ElMessage.success(status === 'enabled' ? t('userApproved') : t('userUpdated'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    approvingUserId.value = null
  }
}

async function confirmDeleteUser(row: User) {
  const confirmed = await confirmDialog(
    t('deleteUserConfirm').replace('{email}', row.email),
    t('confirmDelete'),
    t('delete'),
    'warning'
  )
  if (!confirmed) return
  deletingUserId.value = row.id
  try {
    await deleteUser(row.id)
    ElMessage.success(t('userDeleted'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    deletingUserId.value = null
  }
}

async function searchUsers() {
  resetUsersPagination()
  await reload()
}

async function resetSearch() {
  emailSearch.value = ''
  apiKeySearch.value = ''
  resetUsersPagination()
  await reload()
}

async function nextUsersPage() {
  if (!usersPage.value.has_more || !usersPage.value.next_cursor) return
  usersCursorStack.value[usersCurrentPage.value] = usersPage.value.next_cursor
  usersCurrentPage.value += 1
  jumpTargetPage.value = usersCurrentPage.value
  await reload()
}

async function previousUsersPage() {
  if (usersCurrentPage.value <= 1) return
  usersCurrentPage.value -= 1
  jumpTargetPage.value = usersCurrentPage.value
  await reload()
}

async function handleUsersPageSizeChange(size: number) {
  usersPageSize.value = size
  resetUsersPagination()
  await reload()
}

async function jumpUsersPage() {
  const target = Math.max(1, Math.floor(jumpTargetPage.value || 1))
  jumpTargetPage.value = target
  if (target === usersCurrentPage.value) return
  if (target < usersCurrentPage.value) {
    usersCurrentPage.value = target
    await reload()
    return
  }

  while (usersCurrentPage.value < target) {
    if (!usersPage.value.has_more || !usersPage.value.next_cursor) {
      jumpTargetPage.value = usersCurrentPage.value
      ElMessage.warning(t('targetPageUnavailable'))
      return
    }
    await nextUsersPage()
  }
}

function exportUsers() {
  const header = [
    'id',
    'email',
    'group',
    'status',
    'balance_micro_usd',
    'reserved_micro_usd',
    'available_micro_usd',
    'created_at',
    'last_active_at'
  ]
  const rows = users.value.map((user) => [
    user.id,
    user.email,
    user.user_group_name,
    user.status,
    user.balance_micro_usd,
    user.reserved_micro_usd,
    user.available_micro_usd,
    user.created_at,
    user.last_active_at ?? ''
  ])
  const csv = [header, ...rows]
    .map((row) => row.map((value) => `"${String(value).replace(/"/g, '""')}"`).join(','))
    .join('\n')
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `users-page-${usersCurrentPage.value}.csv`
  link.click()
  URL.revokeObjectURL(url)
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

async function withUserKeysLoading(task: () => Promise<void>) {
  userKeysLoading.value = true
  try {
    await task()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    userKeysLoading.value = false
  }
}

async function nextUserKeysPage() {
  if (!userKeysHasMore.value || !userKeysNextCursor.value) return
  userKeysCursorStack.value[userKeysCurrentPage.value] = userKeysNextCursor.value
  userKeysCurrentPage.value += 1
  await withUserKeysLoading(loadSelectedUserKeys)
}

async function previousUserKeysPage() {
  if (userKeysCurrentPage.value <= 1) return
  userKeysCurrentPage.value -= 1
  await withUserKeysLoading(loadSelectedUserKeys)
}

async function loadUserGroups() {
  try {
    userGroups.value = await getUserGroups()
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function loadServicePolicy() {
  try {
    servicePolicy.value = await getAdminServicePolicy()
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

onMounted(() => {
  void Promise.all([loadUserGroups(), loadServicePolicy()])
})
</script>

<template>
  <section class="grid user-management-view">
    <el-form class="user-toolbar" @submit.prevent="searchUsers">
      <div class="user-toolbar-filters">
        <el-input
          v-model="emailSearch"
          class="user-search-input"
          clearable
          :prefix-icon="Search"
          :placeholder="t('userEmailSearchPlaceholder')"
          @clear="searchUsers"
        />
        <el-input
          v-model="apiKeySearch"
          class="user-search-input is-key-search"
          clearable
          show-password
          :prefix-icon="Key"
          :placeholder="t('apiKeySearchPlaceholder')"
          @clear="searchUsers"
        />
        <el-button
          class="admin-action-button user-search-button"
          type="primary"
          native-type="submit"
          :icon="Search"
          :loading="loading"
        >
          {{ t('search') }}
        </el-button>
        <el-button
          class="admin-action-button user-reset-button"
          :icon="Refresh"
          @click="resetSearch"
        >
          {{ t('reset') }}
        </el-button>
      </div>
      <div class="user-toolbar-actions">
        <el-button class="admin-action-button" :icon="Download" @click="exportUsers">
          {{ t('exportUsers') }}
        </el-button>
        <el-button
          class="admin-action-button add-user-action"
          type="primary"
          :icon="Plus"
          @click="openCreateDialog"
        >
          {{ t('addUser') }}
        </el-button>
      </div>
    </el-form>

    <div v-if="usersInitialLoading" v-loading="true" class="service-table-panel user-table-loading">
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
        <el-table-column prop="id" label="ID" width="76" align="right" header-align="right" />
        <el-table-column prop="email" :label="t('email')" min-width="220">
          <template #default="{ row }">
            <span class="user-email-cell">
              <span class="user-avatar">
                <el-icon><UserFilled /></el-icon>
              </span>
              <span class="user-email-stack">
                <span class="user-email-text">{{ row.email }}</span>
                <span class="user-key-count-text">
                  {{ row.user_key_count.toLocaleString(locale) }} {{ t('apiKey') }}
                </span>
              </span>
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('userGroup')" min-width="130">
          <template #default="{ row }">
            <span class="user-group-tag" :class="`is-${userGroupTone(row)}`">
              {{ row.user_group_name }}
            </span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('availableCredit')"
          min-width="132"
          align="right"
          header-align="right"
        >
          <template #default="{ row }">
            <el-tooltip :content="creditTooltip(row)" placement="top">
              <span class="user-credit-cell" :class="creditCellClass(row)">
                {{ formatAvailableUsd(row) }}
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column :label="t('status')" min-width="128" align="center" header-align="center">
          <template #default="{ row }">
            <span
              class="channel-runtime-status user-status-tag"
              :class="`is-${userStatusTone(row.status)}`"
            >
              <el-icon><component :is="userStatusIcon(row.status)" /></el-icon>
              {{ userStatusText(row.status) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="160">
          <template #default="{ row }">
            <span class="user-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('lastActiveAt')" min-width="190">
          <template #default="{ row }">
            <span class="user-time-cell" :class="{ 'is-empty': !row.last_active_at }">
              {{ formatLastActiveAt(row.last_active_at) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="204" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button
                v-if="row.status === 'pending'"
                class="admin-action-button icon-only-action user-status-action"
                :aria-label="t('approve')"
                :icon="CircleCheckFilled"
                :loading="approvingUserId === row.id"
                @click="approveUser(row)"
              />
              <el-tooltip :content="t('viewApiKeys')" placement="top">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('viewApiKeys')"
                  :icon="Key"
                  @click="openUserKeysDialog(row)"
                />
              </el-tooltip>
              <el-tooltip :content="t('edit')" placement="top">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('edit')"
                  :icon="Edit"
                  @click="openEditDialog(row)"
                />
              </el-tooltip>
              <el-tooltip :content="t('recharge')" placement="top">
                <el-button
                  class="admin-action-button icon-only-action user-recharge-action"
                  :aria-label="t('recharge')"
                  :icon="Money"
                  @click="openCreditDialog(row)"
                />
              </el-tooltip>
              <el-tooltip :content="t('delete')" placement="top">
                <el-button
                  class="admin-action-button icon-only-action"
                  type="danger"
                  :aria-label="t('delete')"
                  :icon="Delete"
                  :loading="deletingUserId === row.id"
                  @click="confirmDeleteUser(row)"
                />
              </el-tooltip>
            </div>
          </template>
        </el-table-column>
        <template #empty>
          <div class="channel-empty-state user-empty-state">
            <el-empty
              :description="emailSearch || apiKeySearch ? t('noMatchingUsers') : t('noUsers')"
            >
              <el-button type="primary" :icon="Plus" @click="openCreateDialog">
                {{ t('addUser') }}
              </el-button>
            </el-empty>
          </div>
        </template>
      </el-table>
    </div>

    <div
      v-if="!usersInitialLoading && (hasUserPagination || users.length > 1)"
      class="admin-pagination-bar"
    >
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
        <div class="admin-page-jump-control">
          <span class="admin-page-label">{{ t('jumpToPage') }}</span>
          <el-input-number
            v-model="jumpTargetPage"
            :min="1"
            :precision="0"
            :controls="false"
            class="admin-page-jump"
            @keyup.enter="jumpUsersPage"
          />
          <el-button :disabled="loading" @click="jumpUsersPage">{{ t('go') }}</el-button>
        </div>
      </div>
    </div>

    <el-dialog
      v-model="createDialogVisible"
      class="user-admin-dialog user-edit-dialog"
      :title="t('addUser')"
      width="520px"
    >
      <div class="user-dialog-body">
        <el-form class="user-dialog-form" label-position="top" @submit.prevent="submitCreateUser">
          <el-form-item class="user-dialog-field is-wide" :label="t('email')">
            <el-input v-model="createForm.email" type="email" />
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('status')">
            <el-select v-model="createForm.status" class="user-edit-select">
              <el-option :label="t('enabled')" value="enabled" />
              <el-option :label="t('disabled')" value="disabled" />
              <el-option :label="t('pendingApproval')" value="pending" />
            </el-select>
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('userGroup')">
            <el-select v-model="createForm.userGroupId" class="user-edit-select">
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
          <el-button @click="createDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="createSaving" @click="submitCreateUser">
            {{ t('create') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

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
        <div v-if="selectedUser" class="user-credit-summary">
          <div>
            <span>{{ t('currentBalance') }}</span>
            <strong>{{ formatMicroUsd(selectedUser.balance_micro_usd, 2) }}</strong>
          </div>
          <div>
            <span>{{ t('reservedCredit') }}</span>
            <strong>{{ formatMicroUsd(selectedUser.reserved_micro_usd, 2) }}</strong>
          </div>
          <div>
            <span>{{ t('afterRecharge') }}</span>
            <strong>{{ formatMicroUsd(rechargePreviewMicroUsd, 2) }}</strong>
          </div>
        </div>
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
              <template #default="{ row }">{{ formatAvailableUsd(row) }}</template>
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
.user-search-input {
  width: min(240px, 100%);
}

.user-search-input.is-key-search {
  width: min(280px, 100%);
}

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

.user-email-cell {
  align-items: center;
  display: inline-flex;
  gap: 11px;
  max-width: 100%;
  min-width: 0;
}

.user-avatar {
  align-items: center;
  background: #eef7fd;
  border: 1px solid #cde9f8;
  border-radius: 8px;
  color: var(--brand-blue);
  display: inline-flex;
  flex: 0 0 auto;
  height: 30px;
  justify-content: center;
  width: 30px;
}

.user-email-stack {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.user-email-text {
  color: #1d2129;
  font-size: 14px;
  font-weight: 680;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.user-key-count-text {
  color: #86909c;
  font-size: 12px;
  font-weight: 560;
  line-height: 1.15;
}

.user-group-tag {
  align-items: center;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  display: inline-flex;
  font-size: 12px;
  font-weight: 720;
  min-height: 28px;
  padding: 0 10px;
  white-space: nowrap;
}

.user-group-tag.is-default {
  background: #f8fafc;
  color: #64748b;
}

.user-group-tag.is-standard {
  background: #eef7fd;
  border-color: #cde9f8;
  color: #0f76b8;
}

.user-group-tag.is-premium {
  background: #eff6ff;
  border-color: #bfdbfe;
  color: #1d4ed8;
}

.user-credit-cell {
  align-items: center;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  display: inline-flex;
  font-feature-settings: 'tnum';
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-weight: 760;
  justify-content: flex-end;
  min-height: 28px;
  min-width: 86px;
  padding: 0 10px;
  white-space: nowrap;
}

.user-credit-cell.is-available {
  background: #f8fafc;
  color: #1d2939;
}

.user-credit-cell.is-unlimited {
  background: #f0fdf4;
  border-color: #bbf7d0;
  color: #15803d;
  justify-content: center;
}

.user-credit-cell.is-depleted {
  background: #fff7ed;
  border-color: #fed7aa;
  color: #c2410c;
  justify-content: center;
}

.user-time-cell {
  color: #344054;
  font-size: 13px;
  font-weight: 560;
}

.user-time-cell.is-empty {
  color: #98a2b3;
}

.user-empty-state {
  padding: 30px 0 34px;
}

.user-credit-summary {
  background: #f8fafc;
  border: 1px solid #dbe4ef;
  border-radius: 8px;
  display: grid;
  gap: 0;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  overflow: hidden;
}

.user-credit-summary div {
  display: grid;
  gap: 6px;
  padding: 12px;
}

.user-credit-summary div + div {
  border-left: 1px solid #e3ebf4;
}

.user-credit-summary span {
  color: #667085;
  font-size: 12px;
  font-weight: 640;
}

.user-credit-summary strong {
  color: #1d2939;
  font-feature-settings: 'tnum';
  font-size: 15px;
  font-variant-numeric: tabular-nums;
  font-weight: 760;
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

  .user-credit-summary {
    grid-template-columns: 1fr;
  }

  .user-credit-summary div + div {
    border-left: 0;
    border-top: 1px solid #e3ebf4;
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
