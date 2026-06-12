<script setup lang="ts">
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Delete,
  Download,
  Edit,
  FolderOpened,
  Money,
  Plus,
  Search,
  UserFilled,
  WarningFilled
} from '@element-plus/icons-vue'
import { computed, reactive, ref, type Component } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  createProject,
  deleteProject,
  getProjectMembers,
  getProjects,
  type ProjectPage,
  updateProject
} from '../../api/projects'
import { adjustCredit } from '../../api/userKeys'
import AdminActionTooltip from '../../components/admin/AdminActionTooltip.vue'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useLocale } from '../../composables/useLocale'
import type { Project, ProjectMember, ProjectStatus } from '../../types/admin'
import { readError } from '../../utils/errors'
import {
  downloadCsv,
  formatCompactDateTime,
  formatDateTime,
  formatMicroUsd,
  usdToMicroUsd
} from '../../utils/format'

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
const deletingProjectId = ref<number | null>(null)
const selectedProject = ref<Project | null>(null)
const selectedMembers = ref<ProjectMember[]>([])
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

function openCreateDialog() {
  Object.assign(createForm, {
    name: '',
    ownerUserId: null,
    status: 'enabled'
  })
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

async function openMembersDialog(row: Project) {
  selectedProject.value = row
  membersDialogVisible.value = true
  selectedMembers.value = []
  membersLoading.value = true
  try {
    selectedMembers.value = await getProjectMembers(row.id)
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    membersLoading.value = false
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

function exportProjects() {
  const header = [
    'id',
    'name',
    'owner_email',
    'status',
    'is_default',
    'member_count',
    'user_key_count',
    'balance_micro_usd',
    'reserved_micro_usd',
    'available_micro_usd',
    'created_at'
  ]
  const rows = projects.value.map((project) => [
    project.id,
    project.name,
    project.owner_email,
    project.status,
    project.is_default ? 'true' : 'false',
    project.member_count,
    project.user_key_count,
    project.balance_micro_usd,
    project.reserved_micro_usd,
    project.available_micro_usd,
    project.created_at
  ])
  downloadCsv(`projects-page-${currentPage.value}.csv`, [header, ...rows])
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
        <el-button class="admin-action-button" :icon="Download" @click="exportProjects">
          {{ t('exportProjects') }}
        </el-button>
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

    <div v-else class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table project-table"
        :data="projects"
        row-key="id"
        stripe
      >
        <el-table-column prop="id" label="ID" width="76" align="right" header-align="right" />
        <el-table-column prop="name" :label="t('project')" min-width="220">
          <template #default="{ row }">
            <span class="project-name-cell">
              <span class="project-avatar">
                <el-icon><FolderOpened /></el-icon>
              </span>
              <span class="project-name-stack">
                <span class="project-name-text">{{ row.name }}</span>
                <span class="project-meta-line">
                  <el-tag v-if="row.is_default" size="small" effect="plain">
                    {{ t('defaultProject') }}
                  </el-tag>
                  <span>{{ row.user_key_count.toLocaleString(locale) }} {{ t('apiKey') }}</span>
                </span>
              </span>
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('projectOwner')" min-width="210">
          <template #default="{ row }">
            <span class="project-owner-cell">
              <el-icon><UserFilled /></el-icon>
              <span>{{ row.owner_email }}</span>
            </span>
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
        <el-table-column :label="t('status')" min-width="128" align="center" header-align="center">
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
        <el-table-column :label="t('createdAt')" min-width="160">
          <template #default="{ row }">
            <span class="user-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="184" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <AdminActionTooltip :content="t('viewProjectMembers')">
                <el-button
                  class="admin-action-button icon-only-action"
                  :aria-label="t('viewProjectMembers')"
                  :icon="UserFilled"
                  @click="openMembersDialog(row)"
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
              <AdminActionTooltip :content="t('recharge')">
                <el-button
                  class="admin-action-button icon-only-action user-recharge-action"
                  :aria-label="t('recharge')"
                  :icon="Money"
                  @click="openCreditDialog(row)"
                />
              </AdminActionTooltip>
              <AdminActionTooltip :content="t('delete')">
                <el-button
                  class="admin-action-button icon-only-action"
                  type="danger"
                  :aria-label="t('delete')"
                  :disabled="row.is_default || row.user_key_count > 0"
                  :icon="Delete"
                  :loading="deletingProjectId === row.id"
                  @click="confirmDeleteProject(row)"
                />
              </AdminActionTooltip>
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
      class="admin-pagination-bar is-compact"
    >
      <div class="admin-pagination-summary">
        <span class="admin-result-count">
          {{ t('currentPageItems') }} {{ projects.length.toLocaleString(locale) }}
          {{ t('itemsUnit') }}
        </span>
      </div>
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
      class="user-admin-dialog project-edit-dialog"
      :title="t('addProject')"
      width="520px"
    >
      <div class="user-dialog-body">
        <el-form class="user-dialog-form" label-position="top" @submit.prevent="submitCreateProject">
          <el-form-item class="user-dialog-field is-wide" :label="t('projectName')">
            <el-input v-model="createForm.name" />
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('ownerUserId')">
            <el-input-number v-model="createForm.ownerUserId" :min="1" :precision="0" />
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('status')">
            <el-select v-model="createForm.status" class="user-edit-select">
              <el-option :label="t('enabled')" value="enabled" />
              <el-option :label="t('disabled')" value="disabled" />
            </el-select>
          </el-form-item>
          <button class="hidden-submit" type="submit" />
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
            <el-input v-model="editForm.name" />
          </el-form-item>
          <el-form-item class="user-dialog-field" :label="t('status')">
            <el-select v-model="editForm.status" class="user-edit-select">
              <el-option :label="t('enabled')" value="enabled" />
              <el-option :label="t('disabled')" value="disabled" />
            </el-select>
          </el-form-item>
          <button class="hidden-submit" type="submit" />
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
      <div class="service-table-panel project-member-panel">
        <el-table
          v-loading="membersLoading"
          class="admin-table service-table"
          :data="selectedMembers"
          row-key="id"
          stripe
        >
          <el-table-column :label="t('email')" min-width="220">
            <template #default="{ row }">
              <span class="project-owner-cell">
                <el-icon><UserFilled /></el-icon>
                <span>{{ row.user_email }}</span>
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('memberRole')" width="120">
            <template #default="{ row }">
              <el-tag effect="plain">{{ memberRoleText(row.role) }}</el-tag>
            </template>
          </el-table-column>
          <el-table-column :label="t('status')" width="112" align="center" header-align="center">
            <template #default="{ row }">
              <el-tag :type="row.user_status === 'enabled' ? 'success' : 'info'" effect="plain">
                {{ row.user_status === 'enabled' ? t('enabled') : t('disabled') }}
              </el-tag>
            </template>
          </el-table-column>
          <el-table-column :label="t('createdAt')" min-width="140">
            <template #default="{ row }">
              <span class="user-time-cell">{{ formatCompactDateTime(row.created_at) }}</span>
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noProjectMembers')" />
          </template>
        </el-table>
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

.project-member-panel {
  max-height: min(58dvh, 520px);
}

@media (max-width: 720px) {
  .project-search-input,
  .project-status-filter {
    width: 100%;
  }
}
</style>
