<script setup lang="ts">
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Delete,
  DocumentCopy,
  Edit,
  FolderOpened,
  Key,
  MoreFilled,
  Money,
  Plus,
  Search,
  User as UserIcon,
  UserFilled,
  WarningFilled
} from '@element-plus/icons-vue'
import { computed, reactive, ref, type Component } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  addProjectMember,
  createProject,
  deleteProject,
  deleteProjectMember,
  getProjectMembers,
  getProjects,
  type ProjectPage,
  updateProjectMember,
  updateProject
} from '../../api/projects'
import { adjustCredit, createProjectUserKey, getUserKeys } from '../../api/userKeys'
import { getUsers } from '../../api/users'
import AdminActionTooltip from '../../components/admin/AdminActionTooltip.vue'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import type { Project, ProjectMember, ProjectStatus, User, UserKey } from '../../types/admin'
import { readError } from '../../utils/errors'
import { formatCompactDateTime, formatDateTime, formatMicroUsd, maskApiKey, usdToMicroUsd } from '../../utils/format'

defineOptions({
  name: 'ProjectsView'
})

const { locale, t } = useLocale()

type TranslationKey = Parameters<typeof t>[0]
type ProjectStatusTone = 'success' | 'neutral'
type ProjectStatusMeta = {
  labelKey: TranslationKey
  icon: Component
  tone: ProjectStatusTone
  confirmType: 'info' | 'warning'
}
type ProjectForm = {
  name: string
  ownerUserId: number | null
  status: ProjectStatus
}
type ProjectKeyForm = {
  scope: 'shared' | 'member'
  memberUserId: number | null
}
type EditableProjectMemberRole = Exclude<ProjectMember['role'], 'owner'>
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
    tone: 'success',
    confirmType: 'info'
  },
  disabled: {
    labelKey: 'disabled',
    icon: WarningFilled,
    tone: 'neutral',
    confirmType: 'warning'
  }
}

const search = ref('')
const statusFilter = ref<ProjectStatus | ''>('')
const createDialogVisible = ref(false)
const createSaving = ref(false)
const editDialogVisible = ref(false)
const editSaving = ref(false)
const creditDialogVisible = ref(false)
const creditSaving = ref(false)
const membersDialogVisible = ref(false)
const membersLoading = ref(false)
const memberSaving = ref(false)
const memberUserOptions = ref<User[]>([])
const memberUserSearchLoading = ref(false)
const updatingMemberId = ref<number | null>(null)
const deletingMemberId = ref<number | null>(null)
const projectKeysDialogVisible = ref(false)
const projectKeysLoading = ref(false)
const projectKeyCreateSaving = ref(false)
const deletingProjectId = ref<number | null>(null)
const selectedProject = ref<Project | null>(null)
const selectedMembers = ref<ProjectMember[]>([])
const selectedProjectKeys = ref<UserKey[]>([])
const createdProjectKey = ref('')
const ownerOptions = ref<User[]>([])
const ownerSearchLoading = ref(false)
const amountUsd = ref(DEFAULT_RECHARGE_USD)
const createForm = reactive<ProjectForm>({
  name: '',
  ownerUserId: null,
  status: 'enabled'
})
const editForm = reactive<Omit<ProjectForm, 'ownerUserId'>>({
  name: '',
  status: 'enabled'
})
const projectKeyForm = reactive<ProjectKeyForm>({
  scope: 'shared',
  memberUserId: null
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
const projects = computed(() => projectsPage.value.items)
const initialLoading = computed(() => !loaded.value)
const hasPagination = computed(() => currentPage.value > 1 || Boolean(projectsPage.value.has_more))
const emptyDescription = computed(() =>
  search.value || statusFilter.value ? t('noMatchingProjects') : t('noProjects')
)
const rechargePreviewMicroUsd = computed(() => {
  if (!selectedProject.value) return usdToMicroUsd(amountUsd.value)
  return selectedProject.value.balance_micro_usd + usdToMicroUsd(amountUsd.value)
})

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

function projectStatusIcon(status: ProjectStatus) {
  return PROJECT_STATUS_META[status].icon
}

function projectStatusTone(status: ProjectStatus): ProjectStatusTone {
  return PROJECT_STATUS_META[status].tone
}

function creditTooltip(row: Project) {
  return [
    `${t('totalCredit')}: ${formatMicroUsd(row.balance_micro_usd, 2)}`,
    `${t('reservedCredit')}: ${formatMicroUsd(row.reserved_micro_usd, 2)}`,
    `${t('remainingCredit')}: ${formatMicroUsd(row.available_micro_usd, 2)}`
  ].join('\n')
}

function creditCellClass(row: Project) {
  return row.available_micro_usd <= 0 ? 'is-depleted' : 'is-available'
}

function formatAvailableUsd(row: Project) {
  return row.available_micro_usd <= 0 ? t('creditDepleted') : formatMicroUsd(row.available_micro_usd, 2)
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

const editableMemberRoleOptions = computed<Array<{ label: string; value: EditableProjectMemberRole }>>(() => [
  { label: memberRoleText('admin'), value: 'admin' },
  { label: memberRoleText('member'), value: 'member' }
])

function projectKeyOwnerText(row: UserKey) {
  if (row.owner_user_id == null) return t('sharedProjectKey')
  const member = selectedMembers.value.find((item) => item.user_id === row.owner_user_id)
  return member ? projectMemberDisplayName(member) : `ID ${row.owner_user_id}`
}

function projectMemberDisplayName(member: ProjectMember) {
  return member.user_username || member.user_email
}

function projectMemberSelectLabel(member: ProjectMember) {
  return member.user_username ? `${member.user_username} / ${member.user_email}` : member.user_email
}

function openCreateDialog() {
  Object.assign(createForm, {
    name: '',
    ownerUserId: null,
    status: 'enabled'
  })
  ownerOptions.value = []
  createDialogVisible.value = true
}

function openEditDialog(row: Project) {
  selectedProject.value = row
  Object.assign(editForm, {
    name: row.name,
    status: row.status
  })
  editDialogVisible.value = true
}

function openCreditDialog(row: Project) {
  selectedProject.value = row
  amountUsd.value = DEFAULT_RECHARGE_USD
  creditDialogVisible.value = true
}

async function openProjectKeysDialog(row: Project) {
  selectedProject.value = row
  projectKeysDialogVisible.value = true
  createdProjectKey.value = ''
  Object.assign(projectKeyForm, {
    scope: 'shared',
    memberUserId: null
  })
  await Promise.all([loadSelectedProjectMembers(), loadSelectedProjectKeys()])
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

async function loadSelectedProjectKeys() {
  if (!selectedProject.value) return
  projectKeysLoading.value = true
  try {
    const page = await getUserKeys({
      projectId: selectedProject.value.id,
      limit: 200
    })
    selectedProjectKeys.value = page.items
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    projectKeysLoading.value = false
  }
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
      customClass: 'admin-confirm-dialog',
      type
    })
    return true
  } catch {
    return false
  }
}

async function submitCreateProject() {
  const name = createForm.name.trim()
  if (!name) {
    ElMessage.error(t('projectNameRequired'))
    return
  }
  if (!createForm.ownerUserId) {
    ElMessage.error(t('projectOwnerRequired'))
    return
  }
  createSaving.value = true
  try {
    await createProject({
      name,
      owner_user_id: createForm.ownerUserId,
      status: createForm.status
    })
    ElMessage.success(t('projectCreated'))
    createDialogVisible.value = false
    await searchProjects()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    createSaving.value = false
  }
}

async function submitCreateProjectKey() {
  if (!selectedProject.value) return
  if (projectKeyForm.scope === 'member' && !projectKeyForm.memberUserId) {
    ElMessage.error(t('projectKeyMemberRequired'))
    return
  }
  projectKeyCreateSaving.value = true
  try {
    const created = await createProjectUserKey(selectedProject.value.id, {
      owner_user_id: projectKeyForm.scope === 'member' ? projectKeyForm.memberUserId : null
    })
    createdProjectKey.value = created.key
    Object.assign(projectKeyForm, {
      scope: 'shared',
      memberUserId: null
    })
    ElMessage.success(t('apiKeyCreated'))
    await loadSelectedProjectKeys()
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    projectKeyCreateSaving.value = false
  }
}

async function copyApiKeyValue(value: string) {
  try {
    await navigator.clipboard.writeText(value)
    ElMessage.success(t('apiKeyCopied'))
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function searchOwnerUsers(query: string) {
  const search = query.trim()
  if (!search) {
    ownerOptions.value = []
    return
  }
  ownerSearchLoading.value = true
  try {
    const page = await getUsers({ search, limit: 20 })
    ownerOptions.value = page.items
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    ownerSearchLoading.value = false
  }
}

async function searchMemberUsers(query: string) {
  const search = query.trim()
  if (!search) {
    memberUserOptions.value = []
    return
  }
  memberUserSearchLoading.value = true
  try {
    const page = await getUsers({ search, limit: 20 })
    memberUserOptions.value = page.items
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    memberUserSearchLoading.value = false
  }
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

async function changeProjectMemberRole(row: ProjectMember, role: EditableProjectMemberRole) {
  if (!selectedProject.value || row.role === 'owner') return
  updatingMemberId.value = row.id
  try {
    await updateProjectMember(selectedProject.value.id, row.id, { role })
    ElMessage.success(t('projectMemberUpdated'))
    await loadSelectedProjectMembers()
  } catch (err) {
    ElMessage.error(readError(err))
    await loadSelectedProjectMembers()
  } finally {
    updatingMemberId.value = null
  }
}

async function confirmDeleteProjectMember(row: ProjectMember) {
  if (!selectedProject.value || row.role === 'owner') return
  const confirmed = await confirmDialog(
    t('deleteProjectMemberConfirm').replace('{email}', row.user_email),
    t('confirmDelete'),
    t('delete'),
    'warning'
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

async function submitEditProject() {
  if (!selectedProject.value) return
  const name = editForm.name.trim()
  if (!name) {
    ElMessage.error(t('projectNameRequired'))
    return
  }
  if (selectedProject.value.status !== editForm.status) {
    const confirmed = await confirmDialog(
      t('changeProjectStatusConfirm')
        .replace('{name}', selectedProject.value.name)
        .replace('{status}', projectStatusText(editForm.status)),
      t('confirmAction'),
      t('save'),
      PROJECT_STATUS_META[editForm.status].confirmType
    )
    if (!confirmed) return
  }
  editSaving.value = true
  try {
    await updateProject(selectedProject.value.id, {
      name,
      status: editForm.status
    })
    ElMessage.success(t('projectUpdated'))
    editDialogVisible.value = false
    await reload()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    editSaving.value = false
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
    'warning'
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

</script>

<template>
  <section class="grid project-management-view">
    <el-form class="user-toolbar project-toolbar" @submit.prevent="searchProjects">
      <div class="user-toolbar-filters">
        <el-input
          v-model="search"
          class="project-search-input"
          clearable
          :prefix-icon="Search"
          :placeholder="t('projectSearchPlaceholder')"
          @clear="searchProjects"
        />
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
      <div class="project-loading-row" />
      <div class="project-loading-row" />
      <div class="project-loading-row" />
    </div>

    <div
      v-else
      class="service-table-panel"
      :class="{ 'has-pagination': hasPagination || projects.length > 1 }"
    >
      <el-table
        v-loading="loading"
        class="admin-table service-table project-table"
        :data="projects"
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
                <span v-if="row.is_default" class="project-meta-line">
                  <el-tag size="small" effect="plain">
                    {{ t('defaultProject') }}
                  </el-tag>
                </span>
              </span>
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('projectOwner')" min-width="210">
          <template #default="{ row }">
            <span class="project-owner-cell">
              <el-icon><UserFilled /></el-icon>
              <span>{{ row.admin_display_names.length ? row.admin_display_names.join(', ') : '-' }}</span>
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
            <el-tooltip :content="creditTooltip(row)" placement="top">
              <span class="user-credit-cell" :class="creditCellClass(row)">
                {{ formatAvailableUsd(row) }}
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="160">
          <template #default="{ row }">
            <span class="user-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('projectStatus')"
          min-width="128"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span
              class="channel-runtime-status user-status-tag"
              :class="`is-${projectStatusTone(row.status)}`"
            >
              <el-icon><component :is="projectStatusIcon(row.status)" /></el-icon>
              {{ projectStatusText(row.status) }}
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="184" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <AdminActionTooltip :content="t('viewProjectMembers')">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('viewProjectMembers')"
                  :icon="UserIcon"
                  @click="openMembersDialog(row)"
                />
              </AdminActionTooltip>
              <AdminActionTooltip :content="t('projectApiKeys')">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('projectApiKeys')"
                  :icon="Key"
                  @click="openProjectKeysDialog(row)"
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
                    <el-dropdown-item @click="openCreditDialog(row)">
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
          <div class="channel-empty-state user-empty-state">
            <el-empty :description="emptyDescription">
              <el-button type="primary" :icon="Plus" @click="openCreateDialog">
                {{ t('addProject') }}
              </el-button>
            </el-empty>
          </div>
        </template>
      </el-table>
    </div>

    <div
      v-if="!initialLoading && (hasPagination || projects.length > 1)"
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
      v-model="createDialogVisible"
      class="user-admin-dialog project-create-dialog"
      :title="t('addProject')"
      width="560px"
    >
      <div class="project-create-body">
        <el-form
          class="project-create-form"
          label-position="top"
          @submit.prevent="submitCreateProject"
        >
          <el-form-item class="project-create-field" :label="t('projectName')">
            <el-input v-model="createForm.name" :placeholder="t('projectNamePlaceholder')" autofocus />
          </el-form-item>
          <div class="project-create-field-row">
            <el-form-item class="project-create-field" :label="t('projectOwner')">
              <el-select
                v-model="createForm.ownerUserId"
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
                    <span>{{ user.email }}</span>
                    <span>{{ user.username || `ID ${user.id}` }}</span>
                  </span>
                </el-option>
              </el-select>
            </el-form-item>
            <el-form-item class="project-create-field" :label="t('projectStatus')">
              <el-select v-model="createForm.status" class="project-create-status-select">
                <el-option :label="t('enabled')" value="enabled" />
                <el-option :label="t('disabled')" value="disabled" />
              </el-select>
            </el-form-item>
          </div>
        </el-form>
      </div>
      <template #footer>
        <div class="admin-dialog-footer user-dialog-footer">
          <el-button @click="createDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="createSaving" @click="submitCreateProject">
            {{ t('create') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="editDialogVisible"
      class="user-admin-dialog project-edit-dialog"
      :title="t('editProject')"
      width="520px"
    >
      <div class="user-dialog-body">
        <el-form class="user-dialog-form" label-position="top" @submit.prevent="submitEditProject">
          <el-form-item class="user-dialog-field is-wide" :label="t('projectName')">
            <el-input v-model="editForm.name" :placeholder="t('projectNamePlaceholder')" />
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('projectStatus')">
            <el-select v-model="editForm.status" class="user-edit-select">
              <el-option :label="t('enabled')" value="enabled" />
              <el-option :label="t('disabled')" value="disabled" />
            </el-select>
          </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <div class="admin-dialog-footer user-dialog-footer">
          <el-button @click="editDialogVisible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="editSaving" @click="submitEditProject">
            {{ t('save') }}
          </el-button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="projectKeysDialogVisible"
      class="user-admin-dialog project-keys-dialog"
      :title="t('projectApiKeys')"
      width="900px"
    >
      <div class="project-keys-dialog-body">
        <el-form
          class="project-key-create-form"
          label-position="top"
          @submit.prevent="submitCreateProjectKey"
        >
          <el-form-item class="project-create-field" :label="t('projectKeyScope')">
            <el-select v-model="projectKeyForm.scope">
              <el-option :label="t('sharedProjectKey')" value="shared" />
              <el-option :label="t('memberPersonalKey')" value="member" />
            </el-select>
          </el-form-item>
          <el-form-item
            v-if="projectKeyForm.scope === 'member'"
            class="project-create-field"
            :label="t('projectMember')"
          >
            <el-select
              v-model="projectKeyForm.memberUserId"
              filterable
              :placeholder="t('projectMemberPlaceholder')"
            >
              <el-option
                v-for="member in selectedMembers"
                :key="member.user_id"
                :label="projectMemberSelectLabel(member)"
                :value="member.user_id"
                :disabled="member.user_status === 'disabled'"
              >
                <span class="project-owner-option">
                  <span>{{ projectMemberDisplayName(member) }}</span>
                  <span>{{ member.user_username ? member.user_email : memberRoleText(member.role) }}</span>
                </span>
              </el-option>
            </el-select>
          </el-form-item>
          <el-form-item class="project-key-create-action">
            <el-button
              class="admin-action-button"
              type="primary"
              :icon="Plus"
              :loading="projectKeyCreateSaving"
              @click="submitCreateProjectKey"
            >
              {{ t('createApiKey') }}
            </el-button>
          </el-form-item>
        </el-form>

        <div v-if="createdProjectKey" class="project-created-key">
          <div>
            <span>{{ t('newApiKey') }}</span>
            <code>{{ createdProjectKey }}</code>
          </div>
          <el-button
            class="admin-action-button"
            :icon="DocumentCopy"
            @click="copyApiKeyValue(createdProjectKey)"
          >
            {{ t('copy') }}
          </el-button>
        </div>

        <div class="service-table-panel project-key-detail-panel">
          <el-table
            v-loading="projectKeysLoading"
            class="admin-table service-table"
            :data="selectedProjectKeys"
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
                      @click="copyApiKeyValue(row.key)"
                    />
                  </el-tooltip>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('keyOwner')" min-width="132">
              <template #default="{ row }">
                {{ projectKeyOwnerText(row) }}
              </template>
            </el-table-column>
            <el-table-column
              :label="t('availableCredit')"
              width="96"
              align="center"
              header-align="center"
            >
              <template #default="{ row }">{{ formatMicroUsd(row.available_micro_usd, 2) }}</template>
            </el-table-column>
            <el-table-column :label="t('status')" width="92" align="center" header-align="center">
              <template #default="{ row }">
                <el-tag class="static-state-tag" :type="row.status === 'enabled' ? 'success' : 'info'">
                  {{ row.status === 'enabled' ? t('enabled') : t('disabled') }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column :label="t('createdAt')" min-width="126">
              <template #default="{ row }">
                <span class="user-time-cell">{{ formatCompactDateTime(row.created_at) }}</span>
              </template>
            </el-table-column>
            <template #empty>
              <el-empty :description="t('noApiKeys')" />
            </template>
          </el-table>
        </div>
      </div>
    </el-dialog>

    <el-dialog
      v-model="creditDialogVisible"
      class="user-admin-dialog user-credit-dialog"
      :title="t('projectBalance')"
      width="460px"
    >
      <div class="user-dialog-body">
        <div v-if="selectedProject" class="user-credit-summary">
          <div>
            <span>{{ t('currentBalance') }}</span>
            <strong>{{ formatMicroUsd(selectedProject.balance_micro_usd, 2) }}</strong>
          </div>
          <div>
            <span>{{ t('reservedCredit') }}</span>
            <strong>{{ formatMicroUsd(selectedProject.reserved_micro_usd, 2) }}</strong>
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
      width="760px"
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
                  <span>{{ user.username || user.email }}</span>
                  <span>{{ user.username ? user.email : `ID ${user.id}` }}</span>
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
            <el-table-column :label="t('username')" min-width="220">
              <template #default="{ row }">
                <span class="project-owner-cell">
                  <el-icon><UserFilled /></el-icon>
                  <span>{{ projectMemberDisplayName(row) }}</span>
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('memberRole')" width="150">
              <template #default="{ row }">
                <el-tag v-if="row.role === 'owner'" class="static-state-tag" effect="plain">
                  {{ memberRoleText(row.role) }}
                </el-tag>
                <el-select
                  v-else
                  class="project-member-role-select"
                  :model-value="row.role"
                  size="small"
                  :loading="updatingMemberId === row.id"
                  @change="(role: EditableProjectMemberRole) => changeProjectMemberRole(row, role)"
                >
                  <el-option
                    v-for="option in editableMemberRoleOptions"
                    :key="option.value"
                    :label="option.label"
                    :value="option.value"
                  />
                </el-select>
              </template>
            </el-table-column>
            <el-table-column :label="t('status')" width="92" align="center" header-align="center">
              <template #default="{ row }">
                <el-tag
                  class="static-state-tag"
                  :type="row.user_status === 'enabled' ? 'success' : 'info'"
                >
                  {{ row.user_status === 'enabled' ? t('enabled') : t('disabled') }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column :label="t('createdAt')" min-width="126">
              <template #default="{ row }">
                <span class="user-time-cell">{{ formatCompactDateTime(row.created_at) }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="96" align="center" header-align="center">
              <template #default="{ row }">
                <AdminActionTooltip v-if="row.role !== 'owner'" :content="t('delete')">
                  <el-button
                    class="admin-icon-action danger"
                    :aria-label="t('delete')"
                    :icon="Delete"
                    :loading="deletingMemberId === row.id"
                    circle
                    text
                    @click="confirmDeleteProjectMember(row)"
                  />
                </AdminActionTooltip>
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

.project-loading-row {
  border-bottom: 1px solid #edf3f8;
  height: 62px;
  position: relative;
}

.project-loading-row::before,
.project-loading-row::after {
  background: #e8eef6;
  border-radius: 999px;
  content: '';
  display: block;
  height: 12px;
  left: 24px;
  position: absolute;
  top: 25px;
}

.project-loading-row::before {
  width: 32px;
}

.project-loading-row::after {
  left: 112px;
  width: min(320px, 40%);
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
  color: #1d2129;
  font-size: 14px;
  font-weight: 700;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-meta-line {
  align-items: center;
  color: #86909c;
  display: inline-flex;
  flex-wrap: wrap;
  font-size: 12px;
  font-weight: 560;
  gap: 6px;
  line-height: 1.15;
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

.project-keys-dialog-body {
  display: grid;
  gap: 14px;
}

.project-key-create-form {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: 168px minmax(260px, 1fr) auto;
}

.project-key-create-form .project-create-field {
  min-width: 0;
}

.project-key-create-action {
  margin-bottom: 0;
}

.project-key-create-action :deep(.el-form-item__content) {
  align-items: end;
}

.project-key-create-action .admin-action-button {
  height: 40px;
  min-height: 40px;
}

.project-created-key {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #dbe4ef;
  border-radius: 8px;
  display: flex;
  gap: 12px;
  justify-content: space-between;
  padding: 12px;
}

.project-created-key div {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.project-created-key span {
  color: #64748b;
  font-size: 12px;
  font-weight: 700;
}

.project-created-key code,
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

.project-created-key code {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-key-detail-panel {
  max-height: min(52dvh, 480px);
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
.project-member-role-field :deep(.el-select),
.project-member-role-select {
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

  .project-key-create-form {
    grid-template-columns: 1fr;
  }

  .project-member-add-form {
    grid-template-columns: 1fr;
  }

  .project-created-key {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
