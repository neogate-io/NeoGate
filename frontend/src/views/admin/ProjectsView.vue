<script setup lang="ts">
import {
  ArrowLeft,
  ArrowRight,
  CircleCheckFilled,
  Delete,
  DocumentCopy,
  Edit,
  FolderOpened,
  Link,
  MagicStick,
  Money,
  Plus,
  Search,
  User as UserIcon,
  UserFilled,
  WarningFilled
} from '@element-plus/icons-vue'
import { computed, onMounted, reactive, ref, type Component, type Ref } from 'vue'
import { ElMessage } from 'element-plus'
import CreditAdjustDialog from '../../components/admin/common/CreditAdjustDialog.vue'
import {
  addProjectMember,
  autoConfigureProjectModel,
  createProjectModel,
  createProject,
  deleteProject,
  deleteProjectMember,
  deleteProjectModel,
  getProjectModels,
  getProjectMembers,
  getProjects,
  type ProjectPage,
  updateProjectModel,
  updateProject
} from '../../api/projects'
import { getChannels } from '../../api/channels'
import { getAdminServicePolicy, type ServicePolicy } from '../../api/policy'
import { adjustCredit } from '../../api/userKeys'
import { getUsers } from '../../api/users'
import { useAsyncData } from '../../composables/useAsyncData'
import { useCursorPageActions } from '../../composables/useCursorPageActions'
import { useCursorPagination } from '../../composables/useCursorPagination'
import { useBillingCurrency } from '../../composables/useBillingCurrency'
import { useLocale } from '../../composables/useLocale'
import { withLoading, withLoadingValue } from '../../composables/useLoadingTask'
import { useReactiveSet } from '../../composables/useReactiveSet'
import type {
  Channel,
  Project,
  ProjectMember,
  ProjectModel,
  AutoSuggestion,
  ProjectModelCandidateTier,
  ProjectStatus,
  User
} from '../../types/admin'
import { copyTextWithMessage } from '../../utils/clipboard'
import { createConfirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import {
  formatCompactDateTime,
  formatDateTime,
  maskApiKey
} from '../../utils/format'

defineOptions({
  name: 'ProjectsView'
})

const { locale, t } = useLocale()
const { formatMoney, majorToMicroAmount } = useBillingCurrency()
const confirmDialog = createConfirmAction(() => t('cancel'))

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
type ProjectModelForm = {
  model: string
  targetModel: string
  targetChannelId: number | null
}
type ProjectModelCandidateForm = {
  targetModel: string
  targetChannelId: number | null
  targetChannelName?: string | null
  createdAt?: string | null
  tier: ProjectModelCandidateTier
  priority: number
  weight: number
  enabled: boolean
}

const DEFAULT_PAGE_SIZE = 50
const DEFAULT_RECHARGE_AMOUNT = 0
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
const modelsDialogVisible = ref(false)
const projectModelActiveTab = ref<'models' | 'smart'>('models')
const membersLoading = ref(false)
const projectModelsLoading = ref(false)
const memberSaving = ref(false)
const projectModelSaving = ref(false)
const smartRouteSaving = ref(false)
const smartAutoConfiguring = ref(false)
const smartAutoDialogVisible = ref(false)
const deletingProjectModelName = ref<string | null>(null)
const memberUserOptions = ref<User[]>([])
const memberUserSearchLoading = ref(false)
const deletingMemberId = ref<number | null>(null)
const deletingProjectId = ref<number | null>(null)
const togglingProjectIds = useReactiveSet<number>()
const selectedProject = ref<Project | null>(null)
const selectedMembers = ref<ProjectMember[]>([])
const selectedProjectModels = ref<ProjectModel[]>([])
const editingSmartRoute = ref<ProjectModel | null>(null)
const smartAutoSuggestions = ref<AutoSuggestion[]>([])
const smartAutoWarnings = ref<string[]>([])
const smartAutoSource = ref('')
const channelOptions = ref<Channel[]>([])
const ownerOptions = ref<User[]>([])
const ownerSearchLoading = ref(false)
const amountMajor = ref(DEFAULT_RECHARGE_AMOUNT)
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
const projectModelForm = reactive<ProjectModelForm>({
  model: '',
  targetModel: '',
  targetChannelId: null
})
const smartCandidateForm = reactive<ProjectModelCandidateForm>({
  targetModel: '',
  targetChannelId: null,
  targetChannelName: null,
  tier: 'standard',
  priority: 0,
  weight: 1,
  enabled: true
})
const smartRouteCandidates = ref<ProjectModelCandidateForm[]>([])
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
const channelModelOptions = computed(() => {
  const seen = new Set<string>()
  const options: string[] = []
  for (const channel of channelOptions.value) {
    if (!channel.enabled) continue
    for (const endpoint of channel.endpoints) {
      if (!endpoint.enabled) continue
      for (const model of endpoint.models) {
        if (seen.has(model)) continue
        seen.add(model)
        options.push(model)
      }
    }
  }
  return options.sort((a, b) => a.localeCompare(b))
})
const directProjectModels = computed(() =>
  selectedProjectModels.value.filter((model) => model.route_mode !== 'smart')
)
const smartProjectModel = computed(
  () => selectedProjectModels.value.find((model) => model.route_mode === 'smart') || null
)
const emptyDescription = computed(() =>
  search.value || statusFilter.value ? t('noMatchingProjects') : t('noProjects')
)
const rechargePreviewMicros = computed(() => {
  if (!selectedProject.value) return majorToMicroAmount(amountMajor.value)
  return selectedProject.value.balance_micros + majorToMicroAmount(amountMajor.value)
})
const isCreditRequired = computed(() => servicePolicy.value?.credit_required ?? true)
const isEditingProject = computed(() => Boolean(selectedProject.value))
const projectDialogTitle = computed(() => t(isEditingProject.value ? 'editProject' : 'addProject'))
const projectSubmitText = computed(() => t(isEditingProject.value ? 'save' : 'create'))
const {
  resetAndReload: resetProjectsAndReload,
  nextPage,
  previousPage,
  handlePageSizeChange
} = useCursorPageActions(
  { pageSize, reset: resetCursorPagination, goToNext, goToPrevious },
  () => projectsPage.value,
  reload
)

async function loadProjects() {
  return getProjects({
    search: search.value.trim(),
    status: statusFilter.value,
    limit: pageSize.value,
    cursor: currentCursor.value
  })
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
    `${t('totalCredit')}: ${formatMoney(row.balance_micros, locale.value, 2)}`,
    `${t('reservedCredit')}: ${formatMoney(row.reserved_micros, locale.value, 2)}`,
    `${t('remainingCredit')}: ${formatMoney(row.available_micros, locale.value, 2)}`
  ].join('\n')
}

function creditCellClass(row: Project): CreditClass {
  return !isCreditRequired.value && row.available_micros === 0 ? 'is-unlimited' : 'is-available'
}

function formatAvailableCredit(row: Project) {
  if (!isCreditRequired.value && row.available_micros === 0) return '-'
  return formatMoney(row.available_micros, locale.value, 2)
}

function formatProjectModelCount(row: Project) {
  return row.project_model_count === 0 ? '-' : row.project_model_count.toLocaleString(locale.value)
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
  const name = member.user_username || member.user_email
  return member.role === 'admin' ? `${name}（管理员）` : name
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
  amountMajor.value = DEFAULT_RECHARGE_AMOUNT
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
  await withLoading(membersLoading, async () => {
    try {
      await loadSelectedProjectMembers()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function openModelsDialog(row: Project) {
  selectedProject.value = row
  modelsDialogVisible.value = true
  projectModelActiveTab.value = 'models'
  selectedProjectModels.value = []
  resetProjectModelForm()
  resetSmartModelForm()
  await withLoading(projectModelsLoading, async () => {
    try {
      await Promise.all([loadSelectedProjectModels(), loadProjectModelOptions()])
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function loadSelectedProjectMembers() {
  if (!selectedProject.value) return
  selectedMembers.value = await getProjectMembers(selectedProject.value.id)
}

async function loadSelectedProjectModels() {
  if (!selectedProject.value) return
  selectedProjectModels.value = await getProjectModels(selectedProject.value.id)
  hydrateSmartModelForm()
}

async function loadProjectModelOptions() {
  channelOptions.value = await getChannels()
}

function resetProjectModelForm() {
  Object.assign(projectModelForm, {
    model: '',
    targetModel: '',
    targetChannelId: null
  })
}

function resetSmartModelForm() {
  editingSmartRoute.value = null
  resetSmartCandidateForm()
  smartRouteCandidates.value = []
}

function resetSmartCandidateForm() {
  Object.assign(smartCandidateForm, {
    targetModel: '',
    targetChannelId: null,
    targetChannelName: null,
    createdAt: null,
    tier: 'standard',
    priority: 0,
    weight: 1,
    enabled: true
  })
}

function hydrateSmartModelForm() {
  const row = smartProjectModel.value
  if (!row) {
    resetSmartModelForm()
    return
  }
  editingSmartRoute.value = row
  smartRouteCandidates.value = row.candidates.map((candidate) => ({
    targetModel: candidate.target_model,
    targetChannelId: candidate.target_channel_id ?? null,
    targetChannelName: candidate.target_channel_name ?? null,
    createdAt: candidate.created_at,
    tier: candidate.tier,
    priority: candidate.priority,
    weight: candidate.weight,
    enabled: candidate.enabled
  }))
}

function channelLabel(channel: Channel) {
  return channel.name
}

function defaultSmartRoutingConfig() {
  return {
    smart_model_name: 'auto',
    default_tier: 'standard' as ProjectModelCandidateTier,
    low_confidence_threshold: 0.7,
    classifier_enabled: false,
    classifier_model: null
  }
}

function smartCandidatePayload(items: ProjectModelCandidateForm[]) {
  return items.map((item) => ({
    target_model: item.targetModel.trim(),
    target_channel_id: item.targetChannelId,
    tier: item.tier,
    priority: item.priority,
    weight: item.weight,
    enabled: item.enabled
  }))
}

function tierLabel(tier: ProjectModelCandidateTier) {
  if (tier === 'simple') return t('projectModelTierSimple')
  if (tier === 'advanced') return t('projectModelTierAdvanced')
  return t('projectModelTierStandard')
}

function autoSelectChannelLabel() {
  return t('projectModelAutoSelectChannel')
}

function candidateChannelLabel(candidate: ProjectModelCandidateForm) {
  if (candidate.targetChannelName) return candidate.targetChannelName
  if (!candidate.targetChannelId) return autoSelectChannelLabel()
  const channel = channelOptions.value.find((item) => item.id === candidate.targetChannelId)
  return channel ? channelLabel(channel) : autoSelectChannelLabel()
}

function suggestionChannelLabel(suggestion: AutoSuggestion) {
  if (suggestion.target_channel_name) return suggestion.target_channel_name
  if (!suggestion.target_channel_id) return autoSelectChannelLabel()
  const channel = channelOptions.value.find((item) => item.id === suggestion.target_channel_id)
  return channel ? channelLabel(channel) : autoSelectChannelLabel()
}

function autoConfigSourceLabel(source: string) {
  return source === 'llm' ? t('projectModelAutoConfigSourceLlm') : t('projectModelAutoConfigSourceRules')
}

function autoConfigWarningText(warning: string) {
  if (warning === '当前智能模型已包含简单、标准、高级档位，无需补全。') {
    return t('projectModelAutoConfigAllTiersExist')
  }
  return warning
}

function autoSuggestionReasonText(suggestion: AutoSuggestion) {
  if (suggestion.reason === '适合简单问答和低成本请求') {
    return t('projectModelSuggestionReasonSimple')
  }
  if (suggestion.reason === '适合复杂推理、架构设计和疑难问题') {
    return t('projectModelSuggestionReasonAdvanced')
  }
  if (suggestion.reason === '适合日常代码、结构化输出和中等复杂任务') {
    return t('projectModelSuggestionReasonStandard')
  }
  return suggestion.reason
}

function smartRouteFallbackCandidate(items: ProjectModelCandidateForm[]) {
  return items.find((item) => item.enabled) || items[0] || null
}

async function persistSmartRouteCandidates(items: ProjectModelCandidateForm[], successMessage: string) {
  const projectId = selectedProject.value?.id
  if (!projectId) return false
  const fallback = smartRouteFallbackCandidate(items)
  if (!fallback) {
    ElMessage.error(t('projectModelAtLeastOneCandidate'))
    return false
  }
  const candidates = smartCandidatePayload(items)
  if (candidates.some((candidate) => !candidate.target_model)) {
    ElMessage.error(t('projectModelCandidateRequired'))
    return false
  }
  let saved = false
  await withLoading(smartRouteSaving, async () => {
    try {
      const row = editingSmartRoute.value
      const payload = {
        target_model: fallback.targetModel.trim(),
        target_channel_id: fallback.targetChannelId,
        routing_config: row?.routing_config || defaultSmartRoutingConfig(),
        candidates
      }
      if (row) {
        await updateProjectModel(projectId, row.model, payload)
      } else {
        await createProjectModel(projectId, {
          model: 'auto',
          route_mode: 'smart',
          enabled: true,
          description: '',
          ...payload
        })
      }
      ElMessage.success(successMessage)
      await loadSelectedProjectModels()
      await reload()
      saved = true
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
  return saved
}

async function addSmartRouteCandidate() {
  const targetModel = smartCandidateForm.targetModel.trim()
  if (!targetModel) {
    ElMessage.error(t('projectModelTargetRequired'))
    return
  }
  const nextCandidates = [
    ...smartRouteCandidates.value,
    {
      targetModel,
      targetChannelId: smartCandidateForm.targetChannelId,
      targetChannelName: null,
      createdAt: null,
      tier: smartCandidateForm.tier,
      priority: smartCandidateForm.priority,
      weight: smartCandidateForm.weight,
      enabled: true
    }
  ]
  const saved = await persistSmartRouteCandidates(nextCandidates, t('projectModelCandidateAdded'))
  if (saved) resetSmartCandidateForm()
}

async function requestSmartAutoConfig() {
  const projectId = selectedProject.value?.id
  if (!projectId) return
  await withLoading(smartAutoConfiguring, async () => {
    try {
      const result = await autoConfigureProjectModel(projectId, {
        mode: smartRouteCandidates.value.length > 0 ? 'fill_missing' : 'replace',
        max_candidates_per_tier: 1
      })
      smartAutoSuggestions.value = result.suggestions
      smartAutoWarnings.value = result.warnings
      smartAutoSource.value = result.source
      if (result.suggestions.length === 0) {
        ElMessage.info(
          result.warnings[0]
            ? autoConfigWarningText(result.warnings[0])
            : t('projectModelAutoConfigNoSuggestions')
        )
        return
      }
      smartAutoDialogVisible.value = true
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

function autoSuggestionToCandidate(suggestion: AutoSuggestion): ProjectModelCandidateForm {
  return {
    targetModel: suggestion.target_model,
    targetChannelId: suggestion.target_channel_id ?? null,
    targetChannelName: suggestion.target_channel_name ?? null,
    createdAt: null,
    tier: suggestion.tier,
    priority: 0,
    weight: 1,
    enabled: true
  }
}

async function applySmartAutoConfig() {
  const existingTiers = new Set(smartRouteCandidates.value.map((candidate) => candidate.tier))
  const nextCandidates = [
    ...smartRouteCandidates.value,
    ...smartAutoSuggestions.value
      .filter((suggestion) => !existingTiers.has(suggestion.tier))
      .map(autoSuggestionToCandidate)
  ]
  if (nextCandidates.length === smartRouteCandidates.value.length) {
    ElMessage.info(t('projectModelSuggestedTiersExist'))
    smartAutoDialogVisible.value = false
    return
  }
  const saved = await persistSmartRouteCandidates(nextCandidates, t('projectModelAutoConfigApplied'))
  if (saved) {
    smartAutoDialogVisible.value = false
    smartAutoSuggestions.value = []
    smartAutoWarnings.value = []
    smartAutoSource.value = ''
  }
}

async function removeSmartRouteCandidate(index: number) {
  const row = editingSmartRoute.value
  const projectId = selectedProject.value?.id
  if (!row || !projectId) return
  const candidate = smartRouteCandidates.value[index]
  const nextCandidates = smartRouteCandidates.value.filter((_, candidateIndex) => candidateIndex !== index)
  const message =
    nextCandidates.length === 0
      ? t('projectModelDeleteLastCandidateConfirm')
      : t('projectModelDeleteCandidateConfirm', { model: candidate.targetModel })
  const confirmed = await confirmDialog(message, t('confirmDelete'), {
    confirmText: t('delete'),
    danger: true,
    type: 'warning'
  })
  if (!confirmed) return
  if (nextCandidates.length === 0) {
    await withLoadingValue(deletingProjectModelName, row.model, null, async () => {
      try {
        await deleteProjectModel(projectId, row.model)
        ElMessage.success(t('projectDeleted'))
        await loadSelectedProjectModels()
        await reload()
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
    return
  }
  await persistSmartRouteCandidates(nextCandidates, t('projectModelCandidateDeleted'))
}

async function submitProjectModelForm() {
  const projectId = selectedProject.value?.id
  if (!projectId) return
  const targetModel = projectModelForm.targetModel.trim()
  const model = projectModelForm.model.trim() || targetModel
  if (!targetModel) {
    ElMessage.error(t('projectModelTargetRequired'))
    return
  }
  await withLoading(projectModelSaving, async () => {
    try {
      const payload = {
        model,
        target_model: targetModel,
        target_channel_id: projectModelForm.targetChannelId
      }
      await createProjectModel(projectId, payload)
      ElMessage.success(t('projectCreated'))
      resetProjectModelForm()
      await loadSelectedProjectModels()
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function confirmDeleteProjectModel(row: ProjectModel) {
  const projectId = selectedProject.value?.id
  if (!projectId) return
  const confirmed = await confirmDialog(t('projectModelDeleteConfirm', { model: row.model }), t('confirmDelete'), {
    confirmText: t('delete'),
    danger: true,
    type: 'warning'
  })
  if (!confirmed) return
  await withLoadingValue(deletingProjectModelName, row.model, null, async () => {
    try {
      await deleteProjectModel(projectId, row.model)
      ElMessage.success(t('projectDeleted'))
      await loadSelectedProjectModels()
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
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
      {
        confirmText: t('save'),
        type: PROJECT_STATUS_META[projectForm.status].confirmType
      }
    )
    if (!confirmed) return
  }

  const ownerUserId = projectForm.ownerUserId
  await withLoading(projectSaving, async () => {
    try {
      if (selectedProject.value) {
        await updateProject(selectedProject.value.id, {
          name,
          owner_user_id: ownerUserId,
          status: projectForm.status
        })
        ElMessage.success(t('projectUpdated'))
        await reload()
      } else {
        await createProject({
          name,
          owner_user_id: ownerUserId,
          status: projectForm.status
        })
        ElMessage.success(t('projectCreated'))
        await searchProjects()
      }
      projectDialogVisible.value = false
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function toggleProjectStatus(row: Project) {
  if (togglingProjectIds.has(row.id)) return

  const nextStatus: ProjectStatus = row.status === 'enabled' ? 'disabled' : 'enabled'
  await togglingProjectIds.withItem(row.id, async () => {
    try {
      await updateProject(row.id, { status: nextStatus })
      ElMessage.success(t('projectUpdated'))
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function copyApiKeyValue(value: string) {
  await copyTextWithMessage(value, t('apiKeyCopied'))
}

async function searchUserOptions(query: string, options: Ref<User[]>, loading: Ref<boolean>) {
  const search = query.trim()
  if (!search) {
    options.value = []
    return
  }
  await withLoading(loading, async () => {
    try {
      const page = await getUsers({ search, limit: 20 })
      options.value = page.items
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

function searchOwnerUsers(query: string) {
  return searchUserOptions(query, ownerOptions, ownerSearchLoading)
}

async function searchMemberUsers(query: string) {
  return searchUserOptions(query, memberUserOptions, memberUserSearchLoading)
}

async function submitAddProjectMember() {
  const projectId = selectedProject.value?.id
  if (!projectId) return
  if (!memberForm.userId) {
    ElMessage.error(t('projectMemberRequired'))
    return
  }
  const memberUserId = memberForm.userId
  await withLoading(memberSaving, async () => {
    try {
      await addProjectMember(projectId, {
        user_id: memberUserId,
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
    }
  })
}

async function confirmDeleteProjectMember(row: ProjectMember) {
  if (!selectedProject.value || row.role === 'owner') return
  const confirmed = await confirmDialog(
    t('deleteProjectMemberConfirm').replace('{email}', row.user_email),
    t('confirmDelete'),
    {
      confirmText: t('delete'),
      danger: true,
      type: 'warning'
    }
  )
  if (!confirmed) return
  const projectId = selectedProject.value.id
  await withLoadingValue(deletingMemberId, row.id, null, async () => {
    try {
      await deleteProjectMember(projectId, row.id)
      ElMessage.success(t('projectMemberRemoved'))
      await loadSelectedProjectMembers()
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function submitCredit() {
  const projectId = selectedProject.value?.id
  if (!projectId) return
  await withLoading(creditSaving, async () => {
    try {
      await adjustCredit('project', projectId, majorToMicroAmount(amountMajor.value))
      ElMessage.success(t('creditUpdated'))
      creditDialogVisible.value = false
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function confirmDeleteProject(row: Project) {
  const confirmed = await confirmDialog(
    t('deleteProjectConfirm').replace('{name}', row.name),
    t('confirmDelete'),
    {
      confirmText: t('delete'),
      danger: true,
      type: 'warning'
    }
  )
  if (!confirmed) return
  await withLoadingValue(deletingProjectId, row.id, null, async () => {
    try {
      await deleteProject(row.id)
      ElMessage.success(t('projectDeleted'))
      await reload()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function searchProjects() {
  await resetProjectsAndReload()
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
        <el-table-column prop="id" label="ID" width="64" align="right" header-align="right" />
        <el-table-column prop="name" :label="t('projectName')" min-width="190">
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
        <el-table-column :label="t('projectOwner')" min-width="180">
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
          min-width="86"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span class="project-count-cell">{{ row.member_count.toLocaleString(locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('modelCount')"
          min-width="76"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <span class="project-count-cell">{{ formatProjectModelCount(row) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('availableCredit')"
          min-width="118"
          align="center"
          header-align="center"
        >
          <template #default="{ row }">
            <el-tooltip :content="creditTooltip(row)" placement="top" :show-after="600">
              <span class="project-credit-cell" :class="creditCellClass(row)">
                {{ formatAvailableCredit(row) }}
              </span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column :label="t('createdAt')" min-width="140">
          <template #default="{ row }">
            <span class="project-time-cell">{{ formatDateTime(row.created_at, locale) }}</span>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('projectStatus')"
          min-width="112"
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
        <el-table-column :label="t('actions')" min-width="286" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('viewProjectMembers')"
                :icon="UserIcon"
                @click="openMembersDialog(row)"
              >
                {{ t('actionMembers') }}
              </el-button>
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('projectModels')"
                :icon="Link"
                @click="openModelsDialog(row)"
              >
                {{ t('actionModels') }}
              </el-button>
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('edit')"
                :icon="Edit"
                @click="openEditDialog(row)"
              >
                {{ t('actionEdit') }}
              </el-button>
              <el-button
                v-if="isCreditRequired"
                class="admin-action-button compact-row-action user-recharge-action"
                :aria-label="t('recharge')"
                :icon="Money"
                @click="openCreditDialog(row)"
              >
                {{ t('actionRecharge') }}
              </el-button>
              <el-button
                class="admin-action-button compact-row-action"
                type="danger"
                :aria-label="t('delete')"
                :disabled="row.is_default || deletingProjectId === row.id"
                :icon="Delete"
                @click="confirmDeleteProject(row)"
              >
                {{ t('actionDelete') }}
              </el-button>
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

    <CreditAdjustDialog
      v-if="selectedProject"
      v-model:amount="amountMajor"
      v-model:open="creditDialogVisible"
      :adjusted-balance="rechargePreviewMicros"
      :confirm-text="t('confirmAdjustment')"
      :current-balance="selectedProject.available_micros"
      :hint="t('projectCreditAdjustHint')"
      :saving="creditSaving"
      :subject-label="t('project')"
      :subject-name="selectedProject.name"
      :title="t('projectBalance')"
      @submit="submitCredit"
    />

    <el-dialog
      v-model="modelsDialogVisible"
      class="user-admin-dialog project-models-dialog"
      :title="t('projectModels')"
      width="720px"
    >
      <div class="project-model-panel">
        <el-tabs v-model="projectModelActiveTab" class="project-model-tabs">
          <el-tab-pane :label="t('projectModelDirectTab')" name="models">
            <p class="project-model-help">
              {{ t('projectModelDirectHelp') }}
            </p>
            <el-form
              class="project-model-form"
              label-position="top"
              @submit.prevent="submitProjectModelForm"
            >
              <el-form-item :label="t('projectModelTargetModel')">
                <el-select
                  v-model="projectModelForm.targetModel"
                  class="project-model-wide-select"
                  filterable
                  allow-create
                  default-first-option
                  :placeholder="t('projectModelTargetModelPlaceholder')"
                >
                  <el-option
                    v-for="model in channelModelOptions"
                    :key="model"
                    :label="model"
                    :value="model"
                  />
                </el-select>
              </el-form-item>
              <el-form-item :label="t('projectModelTargetChannelOptional')">
                <el-select
                  v-model="projectModelForm.targetChannelId"
                  clearable
                  filterable
                  :placeholder="t('projectModelAutoChannelPlaceholder')"
                >
                  <el-option
                    v-for="channel in channelOptions"
                    :key="channel.id"
                    :label="channelLabel(channel)"
                    :value="channel.id"
                  />
                </el-select>
              </el-form-item>
              <el-form-item :label="t('projectModelAliasOptional')">
                <el-input
                  v-model="projectModelForm.model"
                  :placeholder="t('projectModelAliasPlaceholder')"
                />
              </el-form-item>
              <div class="project-model-actions">
                <el-button
                  type="primary"
                  :loading="projectModelSaving"
                  @click="submitProjectModelForm"
                >
                  {{ t('create') }}
                </el-button>
              </div>
            </el-form>

            <div
              v-if="projectModelsLoading || directProjectModels.length > 0"
              class="service-table-panel project-model-table-panel"
            >
              <el-table
                v-loading="projectModelsLoading"
                class="admin-table service-table"
                :data="directProjectModels"
                max-height="46vh"
                row-key="id"
                stripe
              >
                <el-table-column
                  :label="t('projectModelAlias')"
                  prop="model"
                  min-width="120"
                  show-overflow-tooltip
                />
                <el-table-column
                  :label="t('projectModelTargetModel')"
                  prop="target_model"
                  min-width="180"
                  show-overflow-tooltip
                />
                <el-table-column
                  :label="t('projectModelTargetChannel')"
                  width="118"
                  show-overflow-tooltip
                >
                  <template #default="{ row }">
                    <span>{{ row.target_channel_name || autoSelectChannelLabel() }}</span>
                  </template>
                </el-table-column>
                <el-table-column :label="t('createdAt')" width="104">
                  <template #default="{ row }">
                    <span class="user-time-cell project-model-time-cell">{{
                      formatCompactDateTime(row.created_at)
                    }}</span>
                  </template>
                </el-table-column>
                <el-table-column :label="t('actions')" width="56" align="center" header-align="center">
                  <template #default="{ row }">
                    <div class="table-row-actions">
                      <el-tooltip :content="t('delete')" placement="top" :show-after="600">
                        <el-button
                          class="admin-action-button compact-row-action project-member-delete-action"
                          type="danger"
                          :aria-label="t('delete')"
                          :icon="Delete"
                          :loading="deletingProjectModelName === row.model"
                          @click="confirmDeleteProjectModel(row)"
                        />
                      </el-tooltip>
                    </div>
                  </template>
                </el-table-column>
              </el-table>
            </div>
          </el-tab-pane>

          <el-tab-pane :label="t('projectModelSmartTab')" name="smart">
            <div class="smart-route-panel">
              <p class="project-model-help">
                {{ t('projectModelSmartHelp') }}
              </p>
              <el-form
                class="smart-model-form"
                label-position="top"
                @submit.prevent="addSmartRouteCandidate"
              >
                <el-form-item :label="t('projectModelTier')">
                  <el-select
                    v-model="smartCandidateForm.tier"
                    :placeholder="t('projectModelTierPlaceholder')"
                  >
                    <el-option :label="t('projectModelTierSimple')" value="simple" />
                    <el-option :label="t('projectModelTierStandard')" value="standard" />
                    <el-option :label="t('projectModelTierAdvanced')" value="advanced" />
                  </el-select>
                </el-form-item>
                <el-form-item :label="t('projectModelTargetModel')">
                  <el-select
                    v-model="smartCandidateForm.targetModel"
                    class="project-model-wide-select"
                    filterable
                    allow-create
                    default-first-option
                    :placeholder="t('projectModelSelectTargetModel')"
                  >
                    <el-option
                      v-for="model in channelModelOptions"
                      :key="model"
                      :label="model"
                      :value="model"
                    />
                  </el-select>
                </el-form-item>
                <el-form-item :label="t('projectModelTargetChannelOptional')">
                  <el-select
                    v-model="smartCandidateForm.targetChannelId"
                    clearable
                    filterable
                    :placeholder="t('projectModelAutoChannelPlaceholder')"
                  >
                    <el-option
                      v-for="channel in channelOptions"
                      :key="channel.id"
                      :label="channelLabel(channel)"
                      :value="channel.id"
                    />
                  </el-select>
                </el-form-item>
                <div class="project-model-actions">
                  <el-button
                    type="primary"
                    :icon="Plus"
                    :loading="smartRouteSaving"
                    @click="addSmartRouteCandidate"
                  >
                    {{ t('projectModelAddCandidate') }}
                  </el-button>
                  <el-button
                    type="primary"
                    :icon="MagicStick"
                    :loading="smartAutoConfiguring"
                    @click="requestSmartAutoConfig"
                  >
                    {{ t('projectModelAutoConfigure') }}
                  </el-button>
                </div>
              </el-form>

              <div
                v-if="projectModelsLoading || smartRouteCandidates.length > 0"
                class="service-table-panel project-model-table-panel"
              >
                <el-table
                  v-loading="projectModelsLoading || smartRouteSaving"
                  class="admin-table service-table"
                  :data="smartRouteCandidates"
                  max-height="46vh"
                  stripe
                >
                  <el-table-column :label="t('projectModelTier')" width="64">
                    <template #default="{ row }">
                      <span>{{ tierLabel(row.tier) }}</span>
                    </template>
                  </el-table-column>
                  <el-table-column
                    :label="t('projectModelTargetModel')"
                    prop="targetModel"
                    width="168"
                    show-overflow-tooltip
                  />
                  <el-table-column
                    :label="t('projectModelTargetChannel')"
                    width="132"
                    show-overflow-tooltip
                  >
                    <template #default="{ row }">
                      <span>{{ candidateChannelLabel(row) }}</span>
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('projectModelPriority')" width="64" align="center">
                    <template #default="{ row }">
                      <span>{{ row.priority }}</span>
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('projectModelWeight')" width="56" align="center">
                    <template #default="{ row }">
                      <span>{{ row.weight }}</span>
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('createdAt')" width="104">
                    <template #default="{ row }">
                      <span class="user-time-cell project-model-time-cell">{{
                        row.createdAt ? formatCompactDateTime(row.createdAt) : '-'
                      }}</span>
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('actions')" width="58" align="center" header-align="center">
                    <template #default="{ $index }">
                      <div class="table-row-actions">
                        <el-tooltip :content="t('delete')" placement="top" :show-after="600">
                          <el-button
                            class="admin-action-button compact-row-action project-member-delete-action"
                            type="danger"
                            :aria-label="t('delete')"
                            :icon="Delete"
                            :loading="deletingProjectModelName === editingSmartRoute?.model"
                            @click="removeSmartRouteCandidate($index)"
                          />
                        </el-tooltip>
                      </div>
                    </template>
                  </el-table-column>
                </el-table>
              </div>
            </div>
          </el-tab-pane>
        </el-tabs>
      </div>
    </el-dialog>

    <el-dialog
      v-model="smartAutoDialogVisible"
      class="user-admin-dialog smart-auto-dialog"
      :title="t('projectModelAutoConfigSuggestions')"
      width="680px"
    >
      <div class="smart-auto-panel">
        <p class="project-model-help">
          {{ t('projectModelAutoConfigHelp', { source: autoConfigSourceLabel(smartAutoSource) }) }}
        </p>
        <el-alert
          v-for="warning in smartAutoWarnings"
          :key="warning"
          :title="autoConfigWarningText(warning)"
          type="warning"
          show-icon
          :closable="false"
        />
        <div class="service-table-panel smart-auto-table-panel">
          <el-table
            class="admin-table service-table"
            :data="smartAutoSuggestions"
            max-height="42vh"
            stripe
          >
            <el-table-column :label="t('projectModelTier')" width="72">
              <template #default="{ row }">
                <span>{{ tierLabel(row.tier) }}</span>
              </template>
            </el-table-column>
            <el-table-column
              :label="t('projectModelRecommendedModel')"
              prop="target_model"
              min-width="160"
              show-overflow-tooltip
            />
            <el-table-column
              :label="t('projectModelTargetChannel')"
              width="112"
              show-overflow-tooltip
            >
              <template #default="{ row }">
                <span>{{ suggestionChannelLabel(row) }}</span>
              </template>
            </el-table-column>
            <el-table-column
              :label="t('projectModelRecommendationReason')"
              min-width="180"
              show-overflow-tooltip
            >
              <template #default="{ row }">
                <span>{{ autoSuggestionReasonText(row) }}</span>
              </template>
            </el-table-column>
          </el-table>
        </div>
      </div>
      <template #footer>
        <el-button @click="smartAutoDialogVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="smartRouteSaving" @click="applySmartAutoConfig">
          {{ t('projectModelApplyConfig') }}
        </el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="membersDialogVisible"
      class="user-admin-dialog project-members-dialog"
      :title="t('projectMembers')"
      width="820px"
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
            max-height="50vh"
            row-key="id"
            stripe
          >
            <el-table-column :label="t('username')" width="180">
              <template #default="{ row }">
                <span class="project-owner-cell">
                  <el-icon><UserFilled /></el-icon>
                  <span>{{ projectMemberDisplayName(row) }}</span>
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('apiKey')" width="260">
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
            <el-table-column :label="t('createdAt')" width="124">
              <template #default="{ row }">
                <span class="user-time-cell project-member-time-cell">{{
                  formatCompactDateTime(row.created_at)
                }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('lastActiveAt')" width="124">
              <template #default="{ row }">
                <span v-if="row.last_active_at" class="user-time-cell project-member-time-cell">{{
                  formatCompactDateTime(row.last_active_at)
                }}</span>
                <span
                  v-else
                  class="user-time-cell project-member-time-cell project-member-empty-time is-empty"
                >
                  {{ t('neverActive') }}
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="56" align="center" header-align="center">
              <template #default="{ row }">
                <div class="table-row-actions">
                  <el-tooltip :content="t('delete')" placement="top" :show-after="600">
                    <el-button
                      v-if="row.role !== 'owner'"
                      class="admin-action-button compact-row-action project-member-delete-action"
                      type="danger"
                      :aria-label="t('delete')"
                      :icon="Delete"
                      :loading="deletingMemberId === row.id"
                      @click="confirmDeleteProjectMember(row)"
                    />
                  </el-tooltip>
                </div>
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

.project-model-panel {
  display: grid;
  gap: 16px;
}

.project-model-help {
  color: #64748b;
  font-size: 13px;
  line-height: 1.6;
  margin: 0 0 14px;
}

.project-model-form {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(160px, 1fr) minmax(160px, 1fr) minmax(120px, 0.8fr) auto;
}

.project-model-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.project-model-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  white-space: nowrap;
}

.project-model-actions .el-button {
  min-width: 78px;
}

.project-model-table-panel {
  margin: 14px 0 0;
}

.smart-route-panel .project-model-table-panel {
  margin-top: 0;
}

.project-model-time-cell {
  white-space: nowrap;
}

.smart-route-panel {
  display: grid;
  gap: 14px;
}

.smart-model-form {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: 76px minmax(160px, 1fr) minmax(160px, 1fr) auto;
}

.smart-model-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.smart-auto-panel {
  display: grid;
  gap: 12px;
}

.smart-auto-table-panel {
  margin: 0;
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

:global(.project-models-dialog .el-dialog__header) {
  border-bottom: 0;
  padding-bottom: 10px;
}

:global(.project-models-dialog .el-dialog__body) {
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

/* 对话框内成员列表需垂直滚动；覆盖 service-table-panel 对内部滚动条的禁用 */
.project-member-detail-panel :deep(.el-scrollbar__wrap) {
  overflow-y: auto !important;
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
  flex: 0 0 auto;
  font-size: 13px;
  max-width: none;
  min-width: 0;
}

.project-member-delete-action.el-button {
  width: 28px;
}

.project-member-time-cell {
  white-space: nowrap;
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
