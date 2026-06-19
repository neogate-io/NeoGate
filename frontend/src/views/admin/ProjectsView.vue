<script setup lang="ts">
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Delete,
  DocumentCopy,
  Edit,
  FolderOpened,
  Money,
  MoreFilled,
  Plus,
  Search,
  User as UserIcon,
  UserFilled,
  WarningFilled
} from '@element-plus/icons-vue'
import { computed, onMounted, reactive, ref, type Component, type Ref } from 'vue'
import { ElMessage } from 'element-plus'
import {
  addProjectMember,
  createProject,
  deleteProject,
  deleteProjectMember,
  getProjectMembers,
  getProjects,
  type ProjectPage,
  updateProject
} from '../../api/projects'
import { getAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import { adjustCredit } from '../../api/userKeys'
import { getUsers } from '../../api/users'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type { Project, ProjectMember, ProjectStatus, User } from '../../types/admin'
import { copyTextToClipboard } from '../../utils/clipboard'
import { confirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import {
  formatCompactDateTime,
  formatDateTime,
  formatMicroUsd,
  maskApiKey,
  usdToMicroUsd
} from '../../utils/format'

defineOptions({
  name: 'ProjectsView'
})

const { locale, t } = useLocale()

type TranslationKey = Parameters<typeof t>[0]
type CreditClass = 'is-available' | 'is-unlimited'
type ProjectStatusMeta = {
  labelKey: TranslationKey
  icon: Component
  confirmType: 'info' | 'warning'
}
type ProjectForm = {
  name: string
  ownerUserId: number | null
  status: ProjectStatus
}
type EditableProjectMemberRole = Extract<ProjectMember['role'], 'admin' | 'member'>
type ProjectMemberForm = {
  userId: number | null
  role: EditableProjectMemberRole
}

const DEFAULT_PAGE_SIZE = 50
const DEFAULT_RECHARGE_USD = 100
const PROJECT_STATUS_META: Record<ProjectStatus, ProjectStatusMeta> = {
  enabled: {
    labelKey: 'enabled',
    icon: CircleCheckFilled,
    confirmType: 'info'
  },
  disabled: {
    labelKey: 'disabled',
    icon: WarningFilled,
    confirmType: 'warning'
  }
}

const search = ref('')
const statusFilter = ref<ProjectStatus | ''>('')
const projectDialogVisible = ref(false)
const projectSaving = ref(false)
const creditDialogVisible = ref(false)
const creditSaving = ref(false)
const membersDialogVisible = ref(false)
const membersLoading = ref(false)
const memberSaving = ref(false)
const memberUserOptions = ref<User[]>([])
const memberUserSearchLoading = ref(false)
const deletingMemberId = ref<number | null>(null)
const deletingProjectId = ref<number | null>(null)
const togglingProjectIds = useReactiveSet<number>()
const selectedProject = ref<Project | null>(null)
const selectedMembers = ref<ProjectMember[]>([])
const ownerOptions = ref<User[]>([])
const ownerSearchLoading = ref(false)
const amountUsd = ref(DEFAULT_RECHARGE_USD)
const servicePolicy = ref<ServicePolicy | null>(null)
const projectForm = reactive<ProjectForm>({
  name: '',
  ownerUserId: null,
  status: 'enabled'
})
const memberForm = reactive<ProjectMemberForm>({
  userId: null,
  role: 'member'
})
const {
  currentPage,
  pageSize,
  currentCursor,
  reset: resetCursorPagination,
  goToNext,
  goToPrevious
} = useCursorPagination(DEFAULT_PAGE_SIZE)
const {
  data: projectsPage,
  loading,
  loaded,
  reload
} = useAsyncData(loadProjects, {
  items: [],
  limit: DEFAULT_PAGE_SIZE,
  next_cursor: null,
  has_more: false
} satisfies ProjectPage)
const initialLoading = computed(() => !loaded.value)
const showProjectsPagination = computed(
  () =>
    !initialLoading.value &&
    (projectsPage.value.items.length > 0 || currentPage.value > 1 || projectsPage.value.has_more)
)
const emptyDescription = computed(() =>
  search.value || statusFilter.value ? t('noMatchingProjects') : t('noProjects')
)
const rechargePreviewMicroUsd = computed(() => {
  if (!selectedProject.value) return usdToMicroUsd(amountUsd.value)
  return selectedProject.value.balance_micro_usd + usdToMicroUsd(amountUsd.value)
})
const isCreditRequired = computed(() => servicePolicy.value?.credit_required ?? true)
const isEditingProject = computed(() => Boolean(selectedProject.value))
const projectDialogTitle = computed(() => t(isEditingProject.value ? 'editProject' : 'addProject'))
const projectSubmitText = computed(() => t(isEditingProject.value ? 'save' : 'create'))

async function loadProjects() {
  return getProjects({
    search: search.value.trim(),
    status: statusFilter.value,
    limit: pageSize.value,
    cursor: currentCursor.value
  })
}

function resetPagination(page = 1) {
  resetCursorPagination(page)
}

function projectStatusText(status: ProjectStatus) {
  return t(PROJECT_STATUS_META[status].labelKey)
}

function projectRowClassName({ row }: { row: Project }) {
  return row.status === 'disabled' ? 'project-row-is-disabled' : ''
}

function projectStatusIcon(status: ProjectStatus) {
  return PROJECT_STATUS_META[status].icon
}

function creditTooltip(row: Project) {
  return [
    `${t('totalCredit')}: ${formatMicroUsd(row.balance_micro_usd, 2)}`,
    `${t('reservedCredit')}: ${formatMicroUsd(row.reserved_micro_usd, 2)}`,
    `${t('remainingCredit')}: ${formatMicroUsd(row.available_micro_usd, 2)}`
  ].join('\n')
}

function creditCellClass(row: Project): CreditClass {
  return !isCreditRequired.value && row.available_micro_usd === 0 ? 'is-unlimited' : 'is-available'
}

function formatAvailableUsd(row: Project) {
  if (!isCreditRequired.value && row.available_micro_usd === 0) return t('unlimitedCredit')
  return formatMicroUsd(row.available_micro_usd, 2)
}

function memberRoleText(role: ProjectMember['role']) {
  const keys: Record<ProjectMember['role'], TranslationKey> = {
    owner: 'memberRoleOwner',
    admin: 'memberRoleAdmin',
    member: 'memberRoleMember',
    viewer: 'memberRoleViewer'
  }
  return t(keys[role])
}

function userSelectLabel(user: User) {
  return user.username ? `${user.username} / ${user.email}` : user.email
}

function userOptionPrimary(user: User) {
  return user.username || user.email
}

function userOptionSecondary(user: User) {
  return user.username ? user.email : `ID ${user.id}`
}

const editableMemberRoleOptions = computed<
  Array<{ label: string; value: EditableProjectMemberRole }>
>(() => [
  { label: memberRoleText('admin'), value: 'admin' },
  { label: memberRoleText('member'), value: 'member' }
])

function projectMemberDisplayName(member: ProjectMember) {
  return member.user_username || member.user_email
}

function openCreateDialog() {
  selectedProject.value = null
  Object.assign(projectForm, {
    name: '',
    ownerUserId: null,
    status: 'enabled'
  })
  ownerOptions.value = []
  projectDialogVisible.value = true
}

function openEditDialog(row: Project) {
  selectedProject.value = row
  Object.assign(projectForm, {
    name: row.name,
    ownerUserId: row.owner_user_id,
    status: row.status
  })
  ownerOptions.value = []
  projectDialogVisible.value = true
  void searchUserOptions(row.owner_email, ownerOptions, ownerSearchLoading)
}

function openCreditDialog(row: Project) {
  selectedProject.value = row
  amountUsd.value = DEFAULT_RECHARGE_USD
  creditDialogVisible.value = true
}

async function openMembersDialog(row: Project) {
  selectedProject.value = row
  membersDialogVisible.value = true
  selectedMembers.value = []
  memberUserOptions.value = []
  Object.assign(memberForm, {
    userId: null,
    role: 'member'
  })
  membersLoading.value = true
  try {
    await loadSelectedProjectMembers()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    membersLoading.value = false
  }
}

async function loadSelectedProjectMembers() {
  if (!selectedProject.value) return
  selectedMembers.value = await getProjectMembers(selectedProject.value.id)
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

async function submitProjectForm() {
  const name = projectForm.name.trim()
  if (!name) {
    ElMessage.error(t('projectNameRequired'))
    return
  }

  if (!projectForm.ownerUserId) {
    ElMessage.error(t('projectOwnerRequired'))
    return
  }

  if (selectedProject.value && selectedProject.value.status !== projectForm.status) {
    const confirmed = await confirmDialog(
      t('changeProjectStatusConfirm')
        .replace('{name}', selectedProject.value.name)
        .replace('{status}', projectStatusText(projectForm.status)),
      t('confirmAction'),
      t('save'),
      PROJECT_STATUS_META[projectForm.status].confirmType
    )
    if (!confirmed) return
  }

  projectSaving.value = true
  try {
    if (selectedProject.value) {
      await updateProject(selectedProject.value.id, {
        name,
        owner_user_id: projectForm.ownerUserId,
        status: projectForm.status
      })
      ElMessage.success(t('projectUpdated'))
      await reload()
    } else {
      await createProject({
        name,
        owner_user_id: projectForm.ownerUserId!,
        status: projectForm.status
      })
      ElMessage.success(t('projectCreated'))
      await searchProjects()
    }
    projectDialogVisible.value = false
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    projectSaving.value = false
  }
}

async function toggleProjectStatus(row: Project) {
  if (togglingProjectIds.has(row.id)) return

  const nextStatus: ProjectStatus = row.status === 'enabled' ? 'disabled' : 'enabled'
  togglingProjectIds.add(row.id)
  try {
    await updateProject(row.id, { status: nextStatus })
    ElMessage.success(t('projectUpdated'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    togglingProjectIds.remove(row.id)
  }
}

async function copyApiKeyValue(value: string) {
  try {
    await copyTextToClipboard(value)
    ElMessage.success(t('apiKeyCopied'))
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function searchUserOptions(query: string, options: Ref<User[]>, loading: Ref<boolean>) {
  const search = query.trim()
  if (!search) {
    options.value = []
    return
  }
  loading.value = true
  try {
    const page = await getUsers({ search, limit: 20 })
    options.value = page.items
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

function searchOwnerUsers(query: string) {
  return searchUserOptions(query, ownerOptions, ownerSearchLoading)
}

async function searchMemberUsers(query: string) {
  return searchUserOptions(query, memberUserOptions, memberUserSearchLoading)
}

async function submitAddProjectMember() {
  if (!selectedProject.value) return
  if (!memberForm.userId) {
    ElMessage.error(t('projectMemberRequired'))
    return
  }
  memberSaving.value = true
  try {
    await addProjectMember(selectedProject.value.id, {
      user_id: memberForm.userId,
      role: memberForm.role
    })
    ElMessage.success(t('projectMemberAdded'))
    Object.assign(memberForm, {
      userId: null,
      role: 'member'
    })
    memberUserOptions.value = []
    await loadSelectedProjectMembers()
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    memberSaving.value = false
  }
}

async function confirmDeleteProjectMember(row: ProjectMember) {
  if (!selectedProject.value || row.role === 'owner') return
  const confirmed = await confirmDialog(
    t('deleteProjectMemberConfirm').replace('{email}', row.user_email),
    t('confirmDelete'),
    t('delete'),
    'warning',
    true
  )
  if (!confirmed) return
  deletingMemberId.value = row.id
  try {
    await deleteProjectMember(selectedProject.value.id, row.id)
    ElMessage.success(t('projectMemberRemoved'))
    await loadSelectedProjectMembers()
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    deletingMemberId.value = null
  }
}

async function submitCredit() {
  if (!selectedProject.value) return
  creditSaving.value = true
  try {
    await adjustCredit('project', selectedProject.value.id, usdToMicroUsd(amountUsd.value))
    ElMessage.success(t('creditUpdated'))
    creditDialogVisible.value = false
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    creditSaving.value = false
  }
}

async function confirmDeleteProject(row: Project) {
  const confirmed = await confirmDialog(
    t('deleteProjectConfirm').replace('{name}', row.name),
    t('confirmDelete'),
    t('delete'),
    'warning',
    true
  )
  if (!confirmed) return
  deletingProjectId.value = row.id
  try {
    await deleteProject(row.id)
    ElMessage.success(t('projectDeleted'))
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    deletingProjectId.value = null
  }
}

async function searchProjects() {
  resetPagination()
  await reload()
}

async function nextPage() {
  if (!projectsPage.value.has_more || !projectsPage.value.next_cursor) return
  goToNext(projectsPage.value.next_cursor)
  await reload()
}

async function previousPage() {
  if (!goToPrevious()) return
  await reload()
}

async function handlePageSizeChange(size: number) {
  pageSize.value = size
  resetPagination()
  await reload()
}

async function loadServicePolicy() {
  try {
    servicePolicy.value = await getAdminServicePolicy()
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

onMounted(loadServicePolicy)
</script>

<template>
  <section class="grid project-management-view">
    <el-form class="project-toolbar" @submit.prevent="searchProjects">
      <div class="project-toolbar-filters">
        <label class="admin-filter-field">
          <span>{{ t('projectName') }}</span>
          <el-input
            v-model="search"
            class="project-search-input"
            clearable
            :prefix-icon="Search"
            :placeholder="t('projectSearchPlaceholder')"
            @clear="searchProjects"
          />
        </label>
        <label class="admin-filter-field">
          <span>{{ t('projectStatus') }}</span>
          <el-select
            v-model="statusFilter"
            class="project-status-filter"
            :placeholder="t('allProjects')"
            @change="searchProjects"
          >
            <el-option :label="t('allProjects')" value="" />
            <el-option :label="t('enabled')" value="enabled" />
            <el-option :label="t('disabled')" value="disabled" />
          </el-select>
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
      <div class="project-toolbar-actions">
        <el-button
          class="admin-action-button"
          type="primary"
          :icon="Plus"
          @click="openCreateDialog"
        >
          {{ t('addProject') }}
        </el-button>
      </div>
    </el-form>

    <div v-if="initialLoading" v-loading="true" class="service-table-panel project-table-loading">
      <div class="project-table-loading-head">
        <span></span>
        <span></span>
        <span></span>
        <span></span>
        <span></span>
        <span></span>
      </div>
      <div class="project-table-loading-row"></div>
      <div class="project-table-loading-row"></div>
      <div class="project-table-loading-row"></div>
    </div>

    <div v-else class="service-table-panel" :class="{ 'has-pagination': showProjectsPagination }">
      <el-table
        v-loading="loading"
        class="admin-table service-table project-table"
        :data="projectsPage.items"
        :row-class-name="projectRowClassName"
        row-key="id"
        stripe
      >
        <el-table-column prop="id" label="ID" width="76" align="right" header-align="right" />
        <el-table-column prop="name" :label="t('projectName')" min-width="220">
          <template #default="{ row }">
            <span class="project-name-cell">
              <span class="project-avatar">
                <el-icon><FolderOpened /></el-icon>
              </span>
              <span class="project-name-stack">
                <span class="project-name-text">{{ row.name }}</span>
              </span>
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('projectOwner')" min-width="210">
          <template #default="{ row }">
            <span class="project-owner-cell">
              <el-icon><UserFilled /></el-icon>
              <span>{{
                row.admin_display_names.length ? row.admin_display_names.join(', ') : '-'
              }}</span>
            </span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('projectMembers')"
          min-width="96"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span class="project-count-cell">{{ row.member_count.toLocaleString(locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('userApiKeyCount')"
          min-width="96"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span class="project-count-cell">{{ row.user_key_count.toLocaleString(locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('availableCredit')"
          min-width="132"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <el-tooltip :content="creditTooltip(row)" placement="top" :show-after="600">
              <span class="project-credit-cell" :class="creditCellClass(row)">
                {{ formatAvailableUsd(row) }}
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="160">
          <template #default="{ row }">
            <span class="project-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('projectStatus')"
          min-width="128"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <button
              type="button"
              class="project-status-switch"
              :class="`is-${row.status}`"
              :disabled="togglingProjectIds.has(row.id)"
              :aria-pressed="row.status === 'enabled'"
              :aria-label="projectStatusText(row.status)"
              @click="toggleProjectStatus(row)"
            >
              <span class="project-status-switch-icon">
                <el-icon><component :is="projectStatusIcon(row.status)" /></el-icon>
              </span>
              <span class="project-status-switch-text">{{ projectStatusText(row.status) }}</span>
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
              <el-tooltip :content="t('viewProjectMembers')" placement="top" :show-after="600">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('viewProjectMembers')"
                  :icon="UserIcon"
                  @click="openMembersDialog(row)"
                />
              </el-tooltip>
              <el-tooltip :content="t('edit')" placement="top" :show-after="600">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('edit')"
                  :icon="Edit"
                  @click="openEditDialog(row)"
                />
              </el-tooltip>
              <el-dropdown trigger="click" placement="bottom-end">
                <el-button
                  class="admin-action-button icon-only-action action-more-button"
                  :aria-label="t('moreActions')"
                  :icon="MoreFilled"
                />
                <template #dropdown>
                  <el-dropdown-menu class="admin-row-action-menu">
                    <el-dropdown-item v-if="isCreditRequired" @click="openCreditDialog(row)">
                      <el-icon><Money /></el-icon>
                      <span>{{ t('recharge') }}</span>
                    </el-dropdown-item>
                    <el-dropdown-item
                      class="is-danger"
                      :disabled="row.is_default || deletingProjectId === row.id"
                      @click="confirmDeleteProject(row)"
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
          <div class="project-empty-state">
            <el-empty :description="emptyDescription" />
          </div>
        </template>
      </el-table>
    </div>

    <div
      v-if="showProjectsPagination"
      class="admin-pagination-bar admin-table-pagination is-compact"
    >
      <div class="admin-pagination-controls">
        <div class="admin-page-size-control">
          <span class="admin-page-label">{{ t('pageSize') }}</span>
          <el-select v-model="pageSize" class="admin-page-size" @change="handlePageSizeChange">
            <el-option :value="20" label="20" />
            <el-option :value="50" label="50" />
            <el-option :value="100" label="100" />
          </el-select>
        </div>
        <div class="admin-page-buttons">
          <el-button
            :aria-label="t('previousPage')"
            :disabled="currentPage <= 1 || loading"
            :icon="ArrowLeft"
            @click="previousPage"
          />
          <span class="admin-page-current">{{ currentPage }}</span>
          <el-button
            :aria-label="t('nextPage')"
            :disabled="!projectsPage.has_more || loading"
            :icon="ArrowRight"
            @click="nextPage"
          />
        </div>
      </div>
    </div>

    <el-dialog
      v-model="projectDialogVisible"
      class="user-admin-dialog project-create-dialog"
      :title="projectDialogTitle"
      width="560px"
    >
      <div class="project-create-body">
        <el-form
          class="project-create-form"
          label-position="top"
          @submit.prevent="submitProjectForm"
        >
          <el-form-item class="project-create-field" :label="t('projectName')">
            <el-input
              v-model="projectForm.name"
              :placeholder="t('projectNamePlaceholder')"
              autofocus
            />
          </el-form-item>
          <div class="project-create-field-row">
            <el-form-item class="project-create-field" :label="t('projectOwner')">
              <el-select
                v-model="projectForm.ownerUserId"
                class="project-owner-select"
                filterable
                remote
                clearable
                :loading="ownerSearchLoading"
                :placeholder="t('projectOwnerPlaceholder')"
                :remote-method="searchOwnerUsers"
              >
                <el-option
                  v-for="user in ownerOptions"
                  :key="user.id"
                  :label="userSelectLabel(user)"
                  :value="user.id"
                  :disabled="user.status === 'disabled'"
                >
                  <span class="project-owner-option">
                    <span>{{ userOptionPrimary(user) }}</span>
                    <span>{{ userOptionSecondary(user) }}</span>
                  </span>
                </el-option>
              </el-select>
            </el-form-item>
            <el-form-item class="project-create-field" :label="t('projectStatus')">
              <el-select v-model="projectForm.status" class="project-create-status-select">
                <el-option :label="t('enabled')" value="enabled" />
                <el-option :label="t('disabled')" value="disabled" />
              </el-select>
            </el-form-item>
          </div>
        </el-form>
      </div>
      <template #footer>
        <div class="admin-dialog-footer user-dialog-footer">
          <el-button @click="projectDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="projectSaving" @click="submitProjectForm">
            {{ projectSubmitText }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="creditDialogVisible"
      class="user-admin-dialog user-credit-dialog project-credit-dialog"
      :title="t('projectBalance')"
      width="440px"
    >
      <div v-if="selectedProject" class="project-credit-dialog-body">
        <div class="project-credit-project-name">
          <span>{{ t('project') }}</span>
          <strong>{{ selectedProject.name }}</strong>
        </div>

        <section class="project-credit-balance-card">
          <div class="project-credit-balance-row">
            <span>{{ t('availableBalance') }}</span>
            <strong>{{ formatMicroUsd(selectedProject.available_micro_usd, 2) }}</strong>
          </div>
        </section>

        <div class="project-credit-amount-section">
          <label class="project-credit-amount-label">{{ t('amountUsd') }}</label>
          <el-input-number
            v-model="amountUsd"
            :controls="false"
            :min="-100000"
            :precision="2"
            :step="1"
          />
          <p class="project-credit-hint">{{ t('projectCreditAdjustHint') }}</p>
        </div>

        <div class="project-credit-result-card">
          <span>{{ t('afterAdjustment') }}</span>
          <strong>{{ formatMicroUsd(rechargePreviewMicroUsd, 2) }}</strong>
        </div>
      </div>
      <template #footer>
        <div class="admin-dialog-footer user-dialog-footer">
          <el-button @click="creditDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="creditSaving" @click="submitCredit">
            {{ t('save') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="membersDialogVisible"
      class="user-admin-dialog project-members-dialog"
      :title="t('projectMembers')"
      width="860px"
    >
      <div class="project-keys-dialog-body project-member-panel">
        <el-form class="project-member-add-form" @submit.prevent="submitAddProjectMember">
          <el-form-item
            :label="t('memberRole')"
            class="project-create-field project-member-role-field"
          >
            <el-select v-model="memberForm.role">
              <el-option
                v-for="option in editableMemberRoleOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
              />
            </el-select>
          </el-form-item>
          <el-form-item
            :label="t('projectMember')"
            class="project-create-field project-member-user-field"
          >
            <el-select
              v-model="memberForm.userId"
              filterable
              remote
              clearable
              :remote-method="searchMemberUsers"
              :loading="memberUserSearchLoading"
              :placeholder="t('projectMemberPlaceholder')"
            >
              <el-option
                v-for="user in memberUserOptions"
                :key="user.id"
                :label="userSelectLabel(user)"
                :value="user.id"
              >
                <span class="project-owner-option">
                  <span>{{ userOptionPrimary(user) }}</span>
                  <span>{{ userOptionSecondary(user) }}</span>
                </span>
              </el-option>
            </el-select>
          </el-form-item>
          <el-form-item class="project-member-add-action">
            <el-button
              class="admin-action-button"
              type="primary"
              native-type="submit"
              :icon="Plus"
              :loading="memberSaving"
            >
              {{ t('addProjectMember') }}
            </el-button>
          </el-form-item>
        </el-form>
        <div class="service-table-panel project-member-detail-panel">
          <el-table
            v-loading="membersLoading"
            class="admin-table service-table"
            :data="selectedMembers"
            row-key="id"
            stripe
          >
            <el-table-column :label="t('username')" width="112">
              <template #default="{ row }">
                <span class="project-owner-cell">
                  <el-icon><UserFilled /></el-icon>
                  <span>{{ projectMemberDisplayName(row) }}</span>
                </span>
              </template>
            </el-table-column>
            <el-table-column
              :label="t('memberRole')"
              width="88"
              align="center"
              header-align="center"
            >
              <template #default="{ row }">
                <el-tag class="static-state-tag" effect="plain">
                  {{ memberRoleText(row.role) }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column :label="t('apiKey')" min-width="250">
              <template #default="{ row }">
                <div v-if="row.api_key" class="user-key-cell project-member-key-cell">
                  <code class="user-key-value">{{ maskApiKey(row.api_key) }}</code>
                  <el-tooltip :content="t('copy')" placement="top" :show-after="600">
                    <el-button
                      class="user-key-copy-button"
                      :aria-label="t('copy')"
                      :icon="DocumentCopy"
                      @click="copyApiKeyValue(row.api_key)"
                    />
                  </el-tooltip>
                </div>
                <span v-else class="user-time-cell is-empty">-</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('createdAt')" width="116">
              <template #default="{ row }">
                <span class="user-time-cell">{{ formatCompactDateTime(row.created_at) }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('lastActiveAt')" width="116">
              <template #default="{ row }">
                <span v-if="row.last_active_at" class="user-time-cell">{{
                  formatCompactDateTime(row.last_active_at)
                }}</span>
                <span v-else class="user-time-cell project-member-empty-time is-empty">
                  {{ t('neverActive') }}
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="76" align="center" header-align="center">
              <template #default="{ row }">
                <el-tooltip
                  v-if="row.role !== 'owner'"
                  :content="t('delete')"
                  placement="top"
                  :show-after="600"
                >
                  <el-button
                    class="admin-icon-action danger"
                    :aria-label="t('delete')"
                    :icon="Delete"
                    :loading="deletingMemberId === row.id"
                    circle
                    text
                    @click="confirmDeleteProjectMember(row)"
                  />
                </el-tooltip>
              </template>
            </el-table-column>
            <template #empty>
              <el-empty :description="t('noProjectMembers')" />
            </template>
          </el-table>
        </div>
      </div>
    </el-dialog>
  </section>
</template>

<style scoped>
.project-search-input {
  width: min(300px, 100%);
}

.project-status-filter {
  width: 150px;
}

.project-table-loading {
  display: grid;
  gap: 0;
  min-height: 236px;
  overflow: hidden;
}

.project-table-loading-head {
  align-items: center;
  background: #f9fbfd;
  border-bottom: 1px solid #dfe8f2;
  display: grid;
  gap: 24px;
  grid-template-columns: 54px minmax(160px, 1fr) 140px 86px 86px 96px;
  height: 48px;
  min-width: 1000px;
  padding: 0 160px 0 14px;
}

.project-table-loading-head span,
.project-table-loading-row::before,
.project-table-loading-row::after,
.project-table-loading-row span {
  background: #e8eef6;
  border-radius: 999px;
  content: '';
  display: block;
  height: 12px;
}

.project-table-loading-head span:nth-child(1) {
  width: 28px;
}
.project-table-loading-head span:nth-child(2) {
  width: 64px;
}
.project-table-loading-head span:nth-child(3) {
  width: 56px;
}
.project-table-loading-head span:nth-child(4) {
  width: 48px;
}
.project-table-loading-head span:nth-child(5) {
  width: 48px;
}
.project-table-loading-head span:nth-child(6) {
  width: 56px;
}

.project-table-loading-row {
  align-items: center;
  border-bottom: 1px solid #edf3f8;
  display: grid;
  gap: 24px;
  grid-template-columns: 54px minmax(160px, 1fr) 140px 86px 86px 96px;
  height: 62px;
  min-width: 1000px;
  padding: 0 160px 0 14px;
}

.project-table-loading-row::before {
  width: 28px;
}

.project-table-loading-row::after {
  width: min(260px, 100%);
}

.project-table-loading-row span {
  width: 58px;
}

.project-empty-state {
  padding: 30px 0 34px;
}

.project-table,
.project-member-detail-panel {
  font-size: 13px;
}

.project-table :deep(.project-row-is-disabled td) {
  background: #f8fafc;
  color: #94a3b8;
}

.project-table
  :deep(
    .project-row-is-disabled
      :is(
        .project-name-text,
        .project-owner-cell,
        .project-owner-cell .el-icon,
        .project-count-cell,
        .project-credit-cell,
        .project-time-cell,
        .project-status-switch-text
      )
  ) {
  color: #94a3b8;
}

.project-table :deep(.project-row-is-disabled :is(.project-avatar, .project-status-switch)) {
  border-color: #e5e7eb;
}

.project-table :deep(.project-row-is-disabled .project-avatar) {
  background: #f1f5f9;
  color: #94a3b8;
}

.project-name-cell,
.project-owner-cell {
  align-items: center;
  display: inline-flex;
  gap: 11px;
  max-width: 100%;
  min-width: 0;
}

.project-avatar {
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

.project-name-stack {
  display: grid;
  gap: 4px;
  min-width: 0;
}

.project-name-text {
  color: #667085;
  font-size: 13px;
  font-weight: 650 !important;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-owner-cell {
  color: #344054;
  font-size: 13px;
  font-weight: 600;
}

.project-owner-cell .el-icon {
  color: #7c8aa0;
  flex: 0 0 auto;
}

.project-count-cell {
  color: #344054;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 700;
}

.project-credit-cell {
  font-feature-settings: 'tnum';
  font-size: 12.5px;
  font-variant-numeric: tabular-nums;
  font-weight: 400;
  white-space: nowrap;
}

.project-time-cell {
  color: #475467;
  font-size: 12.5px;
  font-weight: 500;
  line-height: 1.35;
}

.project-status-switch.is-enabled,
.project-status-switch.is-enabled .project-status-switch-text {
  background: var(--admin-success-bg);
  border-color: var(--admin-success-border);
  color: var(--admin-success);
}

.project-status-switch.is-disabled,
.project-status-switch.is-disabled .project-status-switch-text {
  background: var(--admin-danger-bg);
  border-color: var(--admin-danger-border);
  color: var(--admin-danger);
}

.project-status-switch.is-enabled .project-status-switch-icon {
  background: #22c55e;
}

.project-create-body {
  padding-top: 2px;
}

:global(.project-create-dialog .el-dialog__header) {
  border-bottom: 0;
  padding-bottom: 10px;
}

:global(.project-create-dialog .el-dialog__body) {
  padding-top: 8px;
}

:global(.project-create-dialog .el-dialog__footer) {
  background: transparent;
  border-top: 0;
  padding-top: 4px;
}

.project-create-form {
  display: grid;
  gap: 18px;
}

.project-create-field-row {
  align-items: start;
  display: grid;
  gap: 16px;
  grid-template-columns: minmax(0, 1fr) minmax(180px, 0.72fr);
}

.project-create-field {
  margin-bottom: 0;
  min-width: 0;
}

.project-create-field :deep(.el-form-item__label) {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 720;
  line-height: 1.2;
  margin-bottom: 8px;
  padding: 0;
}

.project-create-field :deep(.el-input),
.project-create-field :deep(.el-input-number),
.project-create-field :deep(.el-select) {
  width: 100%;
}

.project-create-field :deep(.el-input__wrapper),
.project-create-field :deep(.el-input-number),
.project-create-field :deep(.el-select__wrapper) {
  border-radius: 7px;
  min-height: 40px;
}

.project-owner-option {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
  min-width: 0;
}

.project-owner-option span:first-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-owner-option span:last-child {
  color: #8a98aa;
  flex: 0 0 auto;
  font-size: 12px;
}

:global(.project-credit-dialog .el-dialog__body) {
  padding-top: 8px;
}

:global(.project-credit-dialog .el-dialog__footer) {
  padding-top: 8px;
}

:global(.project-credit-dialog .admin-dialog-footer) {
  border-top: 0;
}

.project-credit-dialog-body {
  display: grid;
  gap: 16px;
}

/* Project name row */
.project-credit-project-name {
  align-items: baseline;
  display: flex;
  gap: 8px;
  min-width: 0;
}

.project-credit-project-name span {
  color: #667085;
  font-size: 12.5px;
  font-weight: 600;
  flex: 0 0 auto;
  line-height: 1.2;
}

.project-credit-project-name strong {
  color: #1d2939;
  font-size: 14px;
  font-weight: 650;
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Balance card */
.project-credit-balance-card {
  background: #f8f9fb;
  border-radius: 8px;
  display: grid;
  gap: 8px;
  padding: 14px 16px;
}

.project-credit-balance-row {
  align-items: baseline;
  display: flex;
  justify-content: space-between;
  min-width: 0;
}

.project-credit-balance-row span {
  color: #667085;
  font-size: 12.5px;
  font-weight: 600;
  line-height: 1.2;
}

.project-credit-balance-row strong {
  color: #1d2939;
  font-feature-settings: 'tnum';
  font-size: 15px;
  font-variant-numeric: tabular-nums;
  font-weight: 650;
  line-height: 1.25;
  text-align: right;
}

/* Amount input section */
.project-credit-amount-section {
  display: grid;
  gap: 6px;
}

.project-credit-amount-label {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 650;
  line-height: 1.2;
}

.project-credit-amount-section :deep(.el-input-number) {
  width: 100%;
}

.project-credit-amount-section :deep(.el-input__wrapper) {
  border-radius: 7px;
  min-height: 40px;
}

.project-credit-amount-section :deep(.el-input__inner) {
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.project-credit-hint {
  color: #667085;
  font-size: 12px;
  line-height: 1.5;
  margin: 0;
}

/* Result highlight card */
.project-credit-result-card {
  align-items: center;
  background: #f0f7ff;
  border: 1px solid #b9d9f5;
  border-radius: 8px;
  display: flex;
  justify-content: space-between;
  padding: 14px 16px;
}

.project-credit-result-card span {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 650;
  line-height: 1.2;
}

.project-credit-result-card strong {
  color: #0f76b8;
  font-feature-settings: 'tnum';
  font-size: 18px;
  font-variant-numeric: tabular-nums;
  font-weight: 700;
  line-height: 1.2;
}

.project-keys-dialog-body {
  display: grid;
  gap: 14px;
}

.user-key-value {
  background: #eef3f8;
  border: 1px solid #dbe4ef;
  border-radius: 6px;
  color: #1d2939;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
  font-weight: 650;
  padding: 4px 7px;
}

.user-key-cell {
  align-items: center;
  display: inline-flex;
  gap: 8px;
  max-width: 100%;
  min-width: 0;
}

.user-key-value {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-member-panel {
  display: grid;
  gap: 14px;
}

.project-member-detail-panel {
  max-height: min(52dvh, 480px);
}

.project-member-detail-panel :deep(.el-table__cell) {
  padding-left: 8px;
  padding-right: 8px;
}

.project-member-detail-panel :deep(.el-table__header-wrapper .cell) {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-member-detail-panel .project-owner-cell {
  max-width: 100%;
}

.project-member-detail-panel .project-owner-cell span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-member-key-cell {
  width: 100%;
}

.project-member-key-cell .user-key-value {
  flex: 1 1 auto;
  max-width: 178px;
  min-width: 0;
}

.project-member-empty-time {
  font-size: 12px;
  line-height: 1.2;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-member-empty-time {
  display: block;
}

.project-member-add-form {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: 116px 260px auto;
  justify-content: start;
}

.project-member-add-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.project-member-user-field :deep(.el-select),
.project-member-role-field :deep(.el-select) {
  width: 100%;
}

.project-member-add-action :deep(.el-form-item__content) {
  align-items: end;
}

.project-member-add-action .admin-action-button {
  height: 40px;
  min-width: 112px;
  min-height: 40px;
}

:global(.project-members-dialog .el-tag),
:global(.project-members-dialog .el-tag *) {
  transition: none !important;
}

:global(.project-members-dialog .el-zoom-in-center-enter-active),
:global(.project-members-dialog .el-zoom-in-center-leave-active),
:global(.project-members-dialog .el-fade-in-enter-active),
:global(.project-members-dialog .el-fade-in-leave-active) {
  animation: none !important;
  transition: none !important;
}

@media (max-width: 720px) {
  .project-search-input,
  .project-status-filter {
    width: 100%;
  }

  .project-create-field-row {
    grid-template-columns: 1fr;
  }

  .project-member-add-form {
    grid-template-columns: 1fr;
  }
}
</style>
