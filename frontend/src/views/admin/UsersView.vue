<script setup lang="ts">
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Delete,
  DocumentCopy,
  Download,
  Edit,
  Key,
  Lock,
  Message,
  MoreFilled,
  Plus,
  Search,
  User as UserIcon,
  WarningFilled
} from '@element-plus/icons-vue'
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { getUserGroups, getUserKeys } from '../../api/userKeys'
import {
  createUser,
  deleteUser,
  getUsers,
  type UserPage,
  updateUser,
  updateUserStatus
} from '../../api/users'
import { getAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import AdminActionTooltip from '../../components/admin/AdminActionTooltip.vue'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type { CreditBalance, User, UserGroup, UserKey, UserStatus } from '../../types/admin'
import { confirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import {
  formatCompactDateTime,
  formatDateTime,
  downloadCsv,
  formatMicroUsd,
  maskApiKey
} from '../../utils/format'

const { locale, t } = useLocale()

defineOptions({
  name: 'UsersView'
})

type CreditClass = 'is-available' | 'is-depleted' | 'is-unlimited'
type ConfirmType = 'info' | 'warning'
type UserGroupTone = 'default' | 'premium' | 'standard'
type UserForm = {
  email: string
  username: string
  password: string
  status: UserStatus
  userGroupId: number
}
type UserDialogMode = 'create' | 'edit'
type TranslationKey = Parameters<typeof t>[0]
type UserStatusMeta = {
  labelKey: TranslationKey
  confirmType: ConfirmType
}

const DEFAULT_USER_PAGE_SIZE = 50
const USER_KEY_DIALOG_LIMIT = 100
const PREMIUM_GROUP_PATTERN = /pro|premium|vip|advanced|高级/i
const USER_STATUS_META: Record<UserStatus, UserStatusMeta> = {
  enabled: {
    labelKey: 'enabled',
    confirmType: 'info'
  },
  pending: {
    labelKey: 'pendingApproval',
    confirmType: 'info'
  },
  disabled: {
    labelKey: 'disabled',
    confirmType: 'warning'
  }
}

const emailSearch = ref('')
const apiKeySearch = ref('')
const userDialogVisible = ref(false)
const userDialogMode = ref<UserDialogMode>('create')
const userDialogSaving = ref(false)
const deletingUserId = ref<number | null>(null)
const approvingUserId = ref<number | null>(null)
const togglingUserIds = useReactiveSet<number>()
const selectedUser = ref<User | null>(null)
const userGroups = ref<UserGroup[]>([])
const servicePolicy = ref<ServicePolicy | null>(null)
const userForm = reactive<UserForm>({
  email: '',
  username: '',
  password: '',
  status: 'enabled',
  userGroupId: 0
})
const userKeysDialogVisible = ref(false)
const userKeysLoading = ref(false)
const selectedUserKeys = ref<UserKey[]>([])
const {
  currentPage: usersCurrentPage,
  pageSize: usersPageSize,
  currentCursor: usersCurrentCursor,
  reset: resetUsersCursorPagination,
  goToNext: goToNextUsersPage,
  goToPrevious: goToPreviousUsersPage
} = useCursorPagination(DEFAULT_USER_PAGE_SIZE)
const {
  data: usersPage,
  loading,
  loaded: usersLoaded,
  reload
} = useAsyncData(loadUsers, {
  items: [],
  limit: DEFAULT_USER_PAGE_SIZE,
  next_cursor: null,
  has_more: false
} satisfies UserPage)
const users = computed(() => usersPage.value.items)
const usersInitialLoading = computed(() => !usersLoaded.value)
const isCreditRequired = computed(() => servicePolicy.value?.credit_required ?? true)
const showAccountBalance = computed(() =>
  Boolean(servicePolicy.value?.credit_required || servicePolicy.value?.recharge_enabled)
)
const defaultUserGroupId = computed(
  () => userGroups.value.find((group) => group.is_default)?.id ?? userGroups.value[0]?.id ?? 0
)
const emptyUsersDescription = computed(() =>
  emailSearch.value || apiKeySearch.value ? t('noMatchingUsers') : t('noUsers')
)
const isUserCreateDialog = computed(() => userDialogMode.value === 'create')
const userDialogTitle = computed(() => t(isUserCreateDialog.value ? 'addUser' : 'editUser'))
const userDialogConfirmText = computed(() => t(isUserCreateDialog.value ? 'create' : 'save'))
async function loadUsers() {
  return getUsers({
    email: emailSearch.value.trim(),
    apiKey: apiKeySearch.value.trim(),
    limit: usersPageSize.value,
    cursor: usersCurrentCursor.value
  })
}

function formatAvailableUsd(row: Pick<CreditBalance, 'available_micro_usd'>) {
  if (!isCreditRequired.value) return t('unlimitedCredit')
  if (row.available_micro_usd <= 0) return t('creditDepleted')
  return formatMicroUsd(row.available_micro_usd, 2)
}

function formatAccountBalance(row: Pick<CreditBalance, 'available_micro_usd'>) {
  return formatMicroUsd(row.available_micro_usd, 2)
}

function creditCellClass(row: Pick<CreditBalance, 'available_micro_usd'>): CreditClass {
  if (!isCreditRequired.value) return 'is-unlimited'
  return row.available_micro_usd <= 0 ? 'is-depleted' : 'is-available'
}

function accountBalanceTooltip(row: CreditBalance) {
  return [
    `${t('accountBalance')}: ${formatMicroUsd(row.balance_micro_usd, 2)}`,
    `${t('reservedBalance')}: ${formatMicroUsd(row.reserved_micro_usd, 2)}`,
    `${t('availableBalance')}: ${formatMicroUsd(row.available_micro_usd, 2)}`
  ].join('\n')
}

function formatLastActiveAt(value?: string | null) {
  return value ? formatCompactDateTime(value) : t('neverActive')
}

function userStatusText(status: UserStatus) {
  return t(USER_STATUS_META[status].labelKey)
}

function userGroupTone(row: User): UserGroupTone {
  if (row.user_group_code === 'default') return 'default'
  if (PREMIUM_GROUP_PATTERN.test(`${row.user_group_code} ${row.user_group_name}`)) {
    return 'premium'
  }
  return 'standard'
}

function userRowClassName({ row }: { row: User }) {
  return row.status === 'disabled' ? 'user-row-is-disabled' : ''
}

function fillUserForm(row?: User) {
  Object.assign(userForm, {
    email: row?.email ?? '',
    username: row?.username ?? '',
    password: '',
    status: row?.status ?? 'enabled',
    userGroupId: row?.user_group_id ?? defaultUserGroupId.value
  })
}

function openCreateDialog() {
  selectedUser.value = null
  userDialogMode.value = 'create'
  fillUserForm()
  userDialogVisible.value = true
}

function openEditDialog(row: User) {
  selectedUser.value = row
  userDialogMode.value = 'edit'
  fillUserForm(row)
  userDialogVisible.value = true
}

async function confirmDialog(
  message: string,
  title: string,
  confirmText: string,
  type: 'info' | 'warning',
  danger = false
) {
  return confirmAction(message, title, {
    confirmText,
    cancelText: t('cancel'),
    danger,
    type
  })
}

function userStatusConfirmMessage(email: string, status: UserStatus) {
  return t('changeUserStatusConfirm')
    .replace('{email}', email)
    .replace('{status}', userStatusText(status))
}

function userStatusConfirmType(status: UserStatus): ConfirmType {
  return USER_STATUS_META[status].confirmType
}

function confirmStatusChange(email: string, status: UserStatus) {
  return confirmDialog(
    userStatusConfirmMessage(email, status),
    t('confirmAction'),
    t('save'),
    userStatusConfirmType(status)
  )
}

async function submitUserDialog() {
  if (isUserCreateDialog.value) {
    await submitCreateUser()
    return
  }
  await submitEditUser()
}

async function submitCreateUser() {
  if (!userForm.password) {
    ElMessage.error(t('passwordRequired'))
    return
  }
  if (userForm.password.length < 8) {
    ElMessage.error(t('passwordMinLength'))
    return
  }
  userDialogSaving.value = true
  try {
    const created = await createUser({
      email: userForm.email.trim(),
      username: userForm.username.trim() || null,
      password: userForm.password,
      status: userForm.status
    })
    if (userForm.userGroupId && userForm.userGroupId !== created.user_group_id) {
      await updateUser(created.id, { user_group_id: userForm.userGroupId })
    }
    ElMessage.success(t('userCreated'))
    userDialogVisible.value = false
    await searchUsers()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    userDialogSaving.value = false
  }
}

async function openUserKeysDialog(row: User) {
  selectedUser.value = row
  userKeysDialogVisible.value = true
  selectedUserKeys.value = []
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

async function submitEditUser() {
  if (!selectedUser.value) return
  userDialogSaving.value = true
  try {
    await updateUser(selectedUser.value.id, {
      email: userForm.email.trim(),
      username: userForm.username.trim() || null,
      status: userForm.status,
      user_group_id: userForm.userGroupId
    })
    ElMessage.success(t('userUpdated'))
    userDialogVisible.value = false
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    userDialogSaving.value = false
  }
}

async function approveUser(row: User) {
  await confirmUserStatusChange(row, 'enabled')
}

async function confirmUserStatusChange(row: User, status: UserStatus) {
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

async function toggleUserStatus(row: User) {
  if (togglingUserIds.has(row.id)) return

  const nextStatus: UserStatus = row.status === 'enabled' ? 'disabled' : 'enabled'
  togglingUserIds.add(row.id)
  try {
    await updateUserStatus(row.id, nextStatus)
    ElMessage.success(nextStatus === 'enabled' ? t('userApproved') : t('userUpdated'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    togglingUserIds.remove(row.id)
  }
}

async function confirmDeleteUser(row: User) {
  const confirmed = await confirmDialog(
    t('deleteUserConfirm').replace('{email}', row.email),
    t('confirmDelete'),
    t('delete'),
    'warning',
    true
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
  resetUsersCursorPagination()
  await reload()
}

async function nextUsersPage() {
  if (!usersPage.value.has_more || !usersPage.value.next_cursor) return
  goToNextUsersPage(usersPage.value.next_cursor)
  await reload()
}

async function previousUsersPage() {
  if (!goToPreviousUsersPage()) return
  await reload()
}

async function handleUsersPageSizeChange(size: number) {
  usersPageSize.value = size
  resetUsersCursorPagination()
  await reload()
}

function exportUsers() {
  const header: string[] = [
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
  downloadCsv(`users-page-${usersCurrentPage.value}.csv`, [header, ...rows])
}

async function loadSelectedUserKeys() {
  if (!selectedUser.value) return
  const page = await getUserKeys({
    userId: selectedUser.value.id,
    limit: USER_KEY_DIALOG_LIMIT
  })
  selectedUserKeys.value = page.items
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
        <label class="admin-filter-field">
          <span>{{ t('email') }}</span>
          <el-input
            v-model="emailSearch"
            class="user-search-input"
            clearable
            :prefix-icon="Search"
            :placeholder="t('userEmailSearchPlaceholder')"
            @clear="searchUsers"
          />
        </label>
        <label class="admin-filter-field">
          <span>{{ t('apiKey') }}</span>
          <el-input
            v-model="apiKeySearch"
            class="user-search-input is-key-search"
            clearable
            show-password
            :prefix-icon="Key"
            :placeholder="t('apiKeySearchPlaceholder')"
            @clear="searchUsers"
          />
        </label>
        <el-button
          class="admin-action-button user-search-button"
          type="primary"
          native-type="submit"
          :icon="Search"
          :loading="loading"
        >
          {{ t('search') }}
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
        <span></span>
      </div>
      <div class="user-table-loading-row"></div>
      <div class="user-table-loading-row"></div>
      <div class="user-table-loading-row"></div>
    </div>

    <div v-else class="service-table-panel has-pagination">
      <el-table
        v-loading="loading"
        class="admin-table service-table user-table"
        :data="users"
        :row-class-name="userRowClassName"
        row-key="id"
        stripe
      >
        <el-table-column prop="id" label="ID" width="68" align="right" header-align="right" />
        <el-table-column prop="username" :label="t('username')" min-width="180">
          <template #default="{ row }">
            <span class="user-email-cell">
              <span class="user-avatar">
                <el-icon><UserIcon /></el-icon>
              </span>
              <span class="user-identity-stack">
                <span class="user-username-text" :class="{ 'is-empty': !row.username }">
                  {{ row.username || '-' }}
                </span>
                <span class="user-mobile-email">{{ row.email }}</span>
                <span class="user-mobile-meta">
                  {{ userStatusText(row.status) }} · {{ row.user_key_count.toLocaleString(locale) }}
                  {{ t('keys') }}
                </span>
                <span class="user-mobile-row-actions">
                  <el-button
                    v-if="row.status === 'pending'"
                    class="admin-action-button icon-only-action user-status-action"
                    :aria-label="t('approve')"
                    :icon="CircleCheckFilled"
                    :loading="approvingUserId === row.id"
                    @click="approveUser(row)"
                  />
                  <AdminActionTooltip :content="t('viewApiKeys')">
                    <el-button
                      class="admin-action-button icon-only-action"
                      :aria-label="t('viewApiKeys')"
                      :icon="Key"
                      @click="openUserKeysDialog(row)"
                    />
                  </AdminActionTooltip>
                  <AdminActionTooltip :content="t('edit')">
                    <el-button
                      class="admin-action-button icon-only-action"
                      :aria-label="t('edit')"
                      :icon="Edit"
                      @click="openEditDialog(row)"
                    />
                  </AdminActionTooltip>
                  <el-dropdown trigger="click" placement="bottom-end">
                    <el-button
                      class="admin-action-button icon-only-action action-more-button"
                      :aria-label="t('moreActions')"
                      :icon="MoreFilled"
                    />
                    <template #dropdown>
                      <el-dropdown-menu class="admin-row-action-menu">
                        <el-dropdown-item
                          class="is-danger"
                          :disabled="deletingUserId === row.id"
                          @click="confirmDeleteUser(row)"
                        >
                          <el-icon><Delete /></el-icon>
                          <span>{{ t('delete') }}</span>
                        </el-dropdown-item>
                      </el-dropdown-menu>
                    </template>
                  </el-dropdown>
                </span>
              </span>
            </span>
          </template>
        </el-table-column>
        <el-table-column prop="email" :label="t('email')" min-width="200">
          <template #default="{ row }">
            <span class="user-email-text">{{ row.email }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('userGroup')" min-width="120">
          <template #default="{ row }">
            <span class="user-group-tag" :class="`is-${userGroupTone(row)}`">
              {{ row.user_group_name }}
            </span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('userApiKeyCount')"
          min-width="104"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span class="user-key-count-text">
              {{ row.user_key_count.toLocaleString(locale) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column
          v-if="showAccountBalance"
          :label="t('accountBalance')"
          min-width="124"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <el-tooltip :content="accountBalanceTooltip(row)" placement="top">
              <span class="user-credit-cell" :class="creditCellClass(row)">
                {{ formatAccountBalance(row) }}
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="150">
          <template #default="{ row }">
            <span class="user-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('lastActiveAt')" min-width="170">
          <template #default="{ row }">
            <span class="user-time-cell" :class="{ 'is-empty': !row.last_active_at }">
              {{ formatLastActiveAt(row.last_active_at) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('userStatus')"
          min-width="116"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <button
              type="button"
              class="user-status-switch"
              :class="`is-${row.status}`"
              :disabled="togglingUserIds.has(row.id)"
              :aria-pressed="row.status === 'enabled'"
              :aria-label="userStatusText(row.status)"
              @click="toggleUserStatus(row)"
            >
              <span class="user-status-switch-icon">
                <el-icon>
                  <CircleCheckFilled v-if="row.status === 'enabled'" />
                  <WarningFilled v-else />
                </el-icon>
              </span>
              <span class="user-status-switch-text">{{ userStatusText(row.status) }}</span>
            </button>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('actions')"
          width="150"
          fixed="right"
          align="center"
          header-align="center"
        >
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
              <AdminActionTooltip :content="t('viewApiKeys')">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('viewApiKeys')"
                  :icon="Key"
                  @click="openUserKeysDialog(row)"
                />
              </AdminActionTooltip>
              <AdminActionTooltip :content="t('edit')">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('edit')"
                  :icon="Edit"
                  @click="openEditDialog(row)"
                />
              </AdminActionTooltip>
              <el-dropdown trigger="click" placement="bottom-end">
                <el-button
                  class="admin-action-button icon-only-action action-more-button"
                  :aria-label="t('moreActions')"
                  :icon="MoreFilled"
                />
                <template #dropdown>
                  <el-dropdown-menu class="admin-row-action-menu">
                    <el-dropdown-item
                      class="is-danger"
                      :disabled="deletingUserId === row.id"
                      @click="confirmDeleteUser(row)"
                    >
                      <el-icon><Delete /></el-icon>
                      <span>{{ t('delete') }}</span>
                    </el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
            </div>
          </template>
        </el-table-column>
        <template #empty>
          <div class="channel-empty-state user-empty-state">
            <el-empty :description="emptyUsersDescription" />
          </div>
        </template>
      </el-table>
    </div>

    <div
      v-if="!usersInitialLoading"
      class="admin-pagination-bar admin-table-pagination is-compact"
    >
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
        <div class="admin-page-buttons">
          <el-button
            :aria-label="t('previousPage')"
            :disabled="usersCurrentPage <= 1 || loading"
            :icon="ArrowLeft"
            @click="previousUsersPage"
          />
          <span class="admin-page-current">{{ usersCurrentPage }}</span>
          <el-button
            :aria-label="t('nextPage')"
            :disabled="!usersPage.has_more || loading"
            :icon="ArrowRight"
            @click="nextUsersPage"
          />
        </div>
      </div>
    </div>

    <el-dialog
      v-model="userDialogVisible"
      class="user-admin-dialog user-edit-dialog user-create-dialog"
      :title="userDialogTitle"
      width="420px"
    >
      <div class="user-dialog-body">
        <el-form
          class="user-dialog-form user-create-form"
          label-position="top"
          @submit.prevent="submitUserDialog"
        >
          <el-form-item class="user-dialog-field is-wide" :label="t('email')">
            <el-input
              v-model="userForm.email"
              :placeholder="t('emailPlaceholder')"
              :prefix-icon="Message"
              type="email"
            />
          </el-form-item>
          <el-form-item class="user-dialog-field is-wide" :label="t('username')">
            <el-input
              v-model="userForm.username"
              maxlength="80"
              :placeholder="t('usernamePlaceholder')"
              :prefix-icon="UserIcon"
              show-word-limit
            />
          </el-form-item>
          <el-form-item
            v-if="isUserCreateDialog"
            class="user-dialog-field is-wide"
            :label="t('loginPassword')"
          >
            <el-input
              v-model="userForm.password"
              autocomplete="new-password"
              :placeholder="t('loginPasswordPlaceholder')"
              :prefix-icon="Lock"
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item class="user-dialog-field is-compact" :label="t('userStatus')">
            <el-select v-model="userForm.status" class="user-edit-select">
              <el-option :label="t('enabled')" value="enabled" />
              <el-option :label="t('disabled')" value="disabled" />
              <el-option :label="t('pendingApproval')" value="pending" />
            </el-select>
          </el-form-item>
          <el-form-item class="user-dialog-field is-compact" :label="t('userGroup')">
            <el-select v-model="userForm.userGroupId" class="user-edit-select">
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
          <el-button @click="userDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="userDialogSaving" @click="submitUserDialog">{{
            userDialogConfirmText
          }}</el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="userKeysDialogVisible"
      class="user-admin-dialog user-keys-dialog"
      :title="t('userPassword')"
      width="860px"
    >
      <div class="user-dialog-body user-keys-dialog-body">
        <div class="service-table-panel user-key-detail-panel">
          <el-table
            v-loading="userKeysLoading"
            class="admin-table service-table user-key-detail-table"
            :data="selectedUserKeys"
            row-key="id"
            stripe
          >
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
            <el-table-column :label="t('projectName')" min-width="128">
              <template #default="{ row }">
                <span class="user-key-project-name">{{ row.project_name }}</span>
              </template>
            </el-table-column>
            <el-table-column
              :label="t('availableCredit')"
              width="96"
              align="center"
              header-align="center"
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
  grid-template-columns: 54px minmax(220px, 1fr) 100px 86px 104px 96px;
  height: 48px;
  min-width: 1080px;
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

.user-table-loading-head span:nth-child(6) {
  width: 64px;
}

.user-table-loading-row {
  align-items: center;
  border-bottom: 1px solid #edf3f8;
  display: grid;
  gap: 28px;
  grid-template-columns: 54px minmax(220px, 1fr) 100px 86px 104px 96px;
  height: 62px;
  min-width: 1080px;
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

.user-table :deep(.el-table__body tr.user-row-is-disabled td) {
  background: #f8fafc;
  color: #94a3b8;
}

.user-table :deep(.el-table__body tr.user-row-is-disabled .user-email-text),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-username-text),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-key-count-text),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-group-tag),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-credit-cell),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-time-cell),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-status-switch-text) {
  color: #94a3b8;
}

.user-table :deep(.el-table__body tr.user-row-is-disabled .user-avatar),
.user-table :deep(.el-table__body tr.user-row-is-disabled .user-status-switch) {
  border-color: #e5e7eb;
}

.user-table :deep(.el-table__body tr.user-row-is-disabled .user-avatar) {
  background: #f1f5f9;
  color: #94a3b8;
}

.user-table.admin-table.el-table {
  font-size: 13px;
}

.user-table.admin-table.el-table :deep(.el-table__body .cell) {
  color: #344054;
  font-size: 13px;
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

.user-create-form {
  grid-template-columns: 1fr;
}

.user-dialog-field {
  margin-bottom: 0;
  min-width: 0;
}

.user-dialog-field.is-wide {
  grid-column: 1 / -1;
}

.user-create-form .user-dialog-field.is-compact {
  max-width: 220px;
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

.user-key-project-name {
  color: #344054;
  font-size: 13px;
  font-weight: 500;
}

.user-email-text,
.user-username-text,
.user-key-project-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

.user-identity-stack {
  display: grid;
  gap: 2px;
  min-width: 0;
}

.user-mobile-email {
  color: #667085;
  display: none;
  font-size: 12px;
  font-weight: 500;
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.user-mobile-meta,
.user-mobile-row-actions {
  display: none;
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

.user-email-text {
  color: #1d2129;
  font-size: 13px;
  font-weight: 400;
  line-height: 1.35;
}

.user-username-text {
  color: #667085;
  font-size: 13px;
  font-weight: 650;
  line-height: 1.35;
}

.user-username-text.is-empty {
  color: #98a2b3;
  font-weight: 400;
}

.user-key-count-text {
  color: #667085;
  font-size: 13px;
  font-weight: 500;
  line-height: 1.35;
}

.user-group-tag {
  align-items: center;
  border: 1px solid #dbe4ef;
  border-radius: 999px;
  display: inline-flex;
  font-size: 12px;
  font-weight: 650;
  line-height: 1;
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
  font-feature-settings: 'tnum';
  font-size: 12.5px;
  font-variant-numeric: tabular-nums;
  font-weight: 400;
  white-space: nowrap;
}

.user-credit-cell.is-available {
  color: #1d2939;
}

.user-credit-cell.is-unlimited {
  color: #15803d;
}

.user-credit-cell.is-depleted {
  color: #1d2939;
}

.user-status-switch.is-enabled {
  background: #f0fdf4;
  border-color: #b7eb8f;
  color: #166534;
}

.user-status-switch.is-disabled {
  background: #f8fafc;
  border-color: #e2e8f0;
  color: #64748b;
}

.user-status-switch.is-pending {
  background: #fffbeb;
  border-color: #f7d37a;
  color: #a16207;
}

.user-status-switch.is-enabled .user-status-switch-icon {
  background: #22c55e;
}

.user-status-switch.is-disabled .user-status-switch-icon {
  background: #94a3b8;
}

.user-status-switch.is-pending .user-status-switch-icon {
  background: #f0b400;
}

.user-time-cell {
  color: #475467;
  font-size: 12.5px;
  font-weight: 500;
  line-height: 1.35;
}

.user-time-cell.is-empty {
  color: #98a2b3;
}

.user-table.admin-table.el-table :deep(.el-table__body .cell .user-email-text) {
  font-weight: 400;
}

.user-table.admin-table.el-table :deep(.el-table__body .cell .user-key-count-text),
.user-table.admin-table.el-table :deep(.el-table__body .cell .user-time-cell) {
  font-weight: 500;
}

.user-table.admin-table.el-table :deep(.el-table__body .cell .user-username-text) {
  font-weight: 650;
}

.user-table.admin-table.el-table :deep(.el-table__body .cell .user-username-text.is-empty) {
  font-weight: 400;
}

.user-table.admin-table.el-table :deep(.el-table__body .cell .user-group-tag) {
  font-weight: 650;
}

.user-empty-state {
  padding: 30px 0 34px;
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

:global(.user-create-dialog) {
  max-width: calc(100vw - 32px);
}

:global(.user-keys-dialog .el-dialog__body) {
  padding: 16px 18px 18px;
}

:global(.user-keys-dialog .el-dialog__footer) {
  display: none;
}

@media (max-width: 640px) {
  .user-mobile-email {
    display: block;
  }

  .user-mobile-meta {
    color: #98a2b3;
    display: block;
    font-size: 12px;
    font-weight: 550;
    line-height: 1.25;
  }

  .user-mobile-row-actions {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }

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

  .user-key-detail-panel {
    max-height: 54dvh;
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
