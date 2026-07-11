<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowRight, Close, Download, Search, Tickets } from '@element-plus/icons-vue'
import {
  downloadAdminUsageStatisticsCsv,
  getAdminUsageStatisticsModels,
  getAdminUsageStatisticsOptions,
  getAdminUsageStatisticsProjectMembers,
  getAdminUsageStatisticsProjects,
  getAdminUsageStatisticsUsers,
  type ModelUsageStatistics,
  type ProjectMemberUsageStatistics,
  type ProjectUsageStatistics,
  type UsageStatisticsExportScope,
  type UsageStatisticsPage,
  type UsageStatisticsQuery,
  type UserUsageStatistics
} from '../../api/usage'
import { useAsyncData } from '../../composables/useAsyncData'
import { useBillingCurrency } from '../../composables/useBillingCurrency'
import { useLocale } from '../../composables/useLocale'
import { downloadBlob, formatNumber, toDateKey } from '../../utils/format'

const { locale, t } = useLocale()
const { formatMoney } = useBillingCurrency()
const router = useRouter()

type PrimaryDimension = 'project' | 'user' | 'model'
type DetailTab = 'projects' | 'users' | 'models'

type AttributionFilters = {
  dateRange: string[] | null
  projectQuery: string
  userQuery: string
  model: string
}

type DrilldownContext = {
  project_id?: number
  project_name?: string
  user_id?: number
  user_name?: string
  model?: string
  channel_id?: number
  channel_name?: string
  billing_meter?: 'token' | 'image' | 'video'
}

const DEFAULT_PAGE_SIZE = 20

const filters = reactive<AttributionFilters>({
  dateRange: defaultRange(30),
  projectQuery: '',
  userQuery: '',
  model: ''
})

const primaryDimension = ref<PrimaryDimension>('project')
const detailTab = ref<DetailTab>('users')
const primaryPage = ref(1)
const primaryPageSize = ref(DEFAULT_PAGE_SIZE)
const detailProjectsPage = ref(1)
const detailProjectsPageSize = ref(DEFAULT_PAGE_SIZE)
const detailUsersPage = ref(1)
const detailUsersPageSize = ref(DEFAULT_PAGE_SIZE)
const detailModelsPage = ref(1)
const detailModelsPageSize = ref(DEFAULT_PAGE_SIZE)
const selectedProject = ref<ProjectUsageStatistics | null>(null)
const selectedUser = ref<UserUsageStatistics | null>(null)
const selectedModel = ref<ModelUsageStatistics | null>(null)
const refinements = ref<DrilldownContext>({})
const exporting = ref(false)

const baseQuery = computed<UsageStatisticsQuery>(() => {
  const [start, end] = filters.dateRange ?? []
  return {
    start,
    end,
    project_query: filters.projectQuery.trim() || undefined,
    user_query: filters.userQuery.trim() || undefined,
    model: filters.model || undefined,
    sort: 'cost_desc'
  }
})

const subjectContext = computed<DrilldownContext>(() => {
  if (primaryDimension.value === 'project' && selectedProject.value?.project_id != null) {
    return {
      project_id: selectedProject.value.project_id,
      project_name: selectedProject.value.project_name
    }
  }
  if (primaryDimension.value === 'user' && selectedUser.value?.user_id != null) {
    return {
      user_id: selectedUser.value.user_id,
      user_name: selectedUser.value.user_display_name
    }
  }
  if (primaryDimension.value === 'model' && selectedModel.value) {
    return {
      model: selectedModel.value.model,
      channel_id: selectedModel.value.channel_id ?? undefined,
      channel_name: selectedModel.value.channel_name,
      billing_meter: selectedModel.value.billing_meter
    }
  }
  return {}
})

const selectedContext = computed<DrilldownContext>(() => ({
  ...subjectContext.value,
  ...refinements.value
}))

const detailQuery = computed<UsageStatisticsQuery>(() => ({
  ...baseQuery.value,
  project_id: selectedContext.value.project_id,
  user_id: selectedContext.value.user_id,
  channel_id: selectedContext.value.channel_id,
  model: selectedContext.value.model ?? baseQuery.value.model,
  billing_meter: selectedContext.value.billing_meter ?? baseQuery.value.billing_meter
}))

const detailTabs = computed<DetailTab[]>(() => {
  if (primaryDimension.value === 'project') return ['users', 'models']
  if (primaryDimension.value === 'user') return ['projects', 'models']
  return ['projects', 'users']
})

const activeDetailPage = computed({
  get: () => detailPage(detailTab.value).value,
  set: (value: number) => {
    detailPage(detailTab.value).value = value
  }
})

const activeDetailPageSize = computed({
  get: () => detailPageSize(detailTab.value).value,
  set: (value: number) => {
    detailPageSize(detailTab.value).value = value
  }
})

const hasSelection = computed(
  () =>
    (primaryDimension.value === 'project' && selectedProject.value != null) ||
    (primaryDimension.value === 'user' && selectedUser.value != null) ||
    (primaryDimension.value === 'model' && selectedModel.value != null)
)
const selectedAnalysisTitle = computed(() => {
  if (primaryDimension.value === 'project') return t('projectAnalysis')
  if (primaryDimension.value === 'user') return t('userAnalysis')
  return t('modelAnalysis')
})

const activeQuickRange = computed(() => {
  for (const days of [7, 30, 90]) {
    const [start, end] = defaultRange(days)
    if (filters.dateRange?.[0] === start && filters.dateRange?.[1] === end) return days
  }
  return null
})

const {
  data: primaryProjects,
  loading: primaryProjectsLoading,
  reload: reloadPrimaryProjects
} = useAsyncData(
  () => {
    if (primaryDimension.value !== 'project') return Promise.resolve(emptyPage<ProjectUsageStatistics>())
    return getAdminUsageStatisticsProjects({
      ...baseQuery.value,
      page: primaryPage.value,
      limit: primaryPageSize.value
    })
  },
  emptyPage<ProjectUsageStatistics>()
)

const {
  data: primaryUsers,
  loading: primaryUsersLoading,
  reload: reloadPrimaryUsers
} = useAsyncData(
  () => {
    if (primaryDimension.value !== 'user') return Promise.resolve(emptyPage<UserUsageStatistics>())
    return getAdminUsageStatisticsUsers({
      ...baseQuery.value,
      page: primaryPage.value,
      limit: primaryPageSize.value
    })
  },
  emptyPage<UserUsageStatistics>()
)

const {
  data: primaryModels,
  loading: primaryModelsLoading,
  reload: reloadPrimaryModels
} = useAsyncData(
  () => {
    if (primaryDimension.value !== 'model') return Promise.resolve(emptyPage<ModelUsageStatistics>())
    return getAdminUsageStatisticsModels({
      ...baseQuery.value,
      page: primaryPage.value,
      limit: primaryPageSize.value
    })
  },
  emptyPage<ModelUsageStatistics>()
)

const {
  data: detailProjects,
  loading: detailProjectsLoading,
  reload: reloadDetailProjects
} = useAsyncData(
  () => {
    if (!hasSelection.value || !detailTabs.value.includes('projects')) {
      return Promise.resolve(emptyPage<ProjectUsageStatistics>())
    }
    return getAdminUsageStatisticsProjects({
      ...detailQuery.value,
      page: detailProjectsPage.value,
      limit: detailProjectsPageSize.value
    })
  },
  emptyPage<ProjectUsageStatistics>()
)

const {
  data: detailUsers,
  loading: detailUsersLoading,
  reload: reloadDetailUsers
} = useAsyncData(
  () => {
    if (!hasSelection.value || !detailTabs.value.includes('users')) {
      return Promise.resolve(emptyPage<UserUsageStatistics | ProjectMemberUsageStatistics>())
    }
    const query = {
      ...detailQuery.value,
      page: detailUsersPage.value,
      limit: detailUsersPageSize.value
    }
    if (detailQuery.value.project_id != null) {
      return getAdminUsageStatisticsProjectMembers(query)
    }
    return getAdminUsageStatisticsUsers(query)
  },
  emptyPage<UserUsageStatistics | ProjectMemberUsageStatistics>()
)

const {
  data: detailModels,
  loading: detailModelsLoading,
  reload: reloadDetailModels
} = useAsyncData(
  () => {
    if (!hasSelection.value || !detailTabs.value.includes('models')) {
      return Promise.resolve(emptyPage<ModelUsageStatistics>())
    }
    return getAdminUsageStatisticsModels({
      ...detailQuery.value,
      page: detailModelsPage.value,
      limit: detailModelsPageSize.value
    })
  },
  emptyPage<ModelUsageStatistics>()
)
const {
  data: attributionOptions,
  loading: attributionOptionsLoading,
  reload: reloadAttributionOptions
} = useAsyncData(
  () => getAdminUsageStatisticsOptions(baseQuery.value),
  { models: [], users: [] }
)

const primaryLoading = computed(
  () => primaryProjectsLoading.value || primaryUsersLoading.value || primaryModelsLoading.value
)
const detailLoading = computed(
  () =>
    detailProjectsLoading.value ||
    detailUsersLoading.value ||
    detailModelsLoading.value
)
const loading = computed(
  () => primaryLoading.value || detailLoading.value || attributionOptionsLoading.value
)
const filteredModelOptions = computed(() => attributionOptions.value.models)

async function reloadPrimary() {
  if (primaryDimension.value === 'project') await reloadPrimaryProjects()
  if (primaryDimension.value === 'user') await reloadPrimaryUsers()
  if (primaryDimension.value === 'model') await reloadPrimaryModels()
  syncSelectedFromPrimary()
}

async function reloadDetails() {
  if (!hasSelection.value) return
  const reloads: Array<Promise<void>> = []
  if (detailTabs.value.includes('projects')) reloads.push(reloadDetailProjects())
  if (detailTabs.value.includes('users')) reloads.push(reloadDetailUsers())
  if (detailTabs.value.includes('models')) reloads.push(reloadDetailModels())
  await Promise.all(reloads)
}

async function reloadAttribution() {
  primaryPage.value = 1
  resetDetailPages()
  await reloadAttributionOptions()
  await reloadPrimary()
  await reloadDetails()
}

async function changePrimaryDimension(tab: PrimaryDimension) {
  primaryDimension.value = tab
  primaryPage.value = 1
  clearSelection()
  detailTab.value = defaultDetailTab(tab)
  await reloadPrimary()
}

async function applyQuickRange(days: number) {
  filters.dateRange = defaultRange(days)
  await reloadAttribution()
}

async function selectProject(row: ProjectUsageStatistics) {
  if (row.project_id == null) return
  if (selectedProject.value?.project_id === row.project_id) {
    await clearSelection()
    return
  }
  selectedProject.value = row
  selectedUser.value = null
  selectedModel.value = null
  refinements.value = {}
  detailTab.value = 'users'
  resetDetailPages()
  await reloadDetails()
}

async function selectUser(row: UserUsageStatistics | ProjectMemberUsageStatistics) {
  if (row.user_id == null) return
  if (primaryDimension.value === 'user') {
    if (selectedUser.value?.user_id === row.user_id) {
      await clearSelection()
      return
    }
    selectedUser.value = row as UserUsageStatistics
    selectedProject.value = null
    selectedModel.value = null
    refinements.value = {}
    detailTab.value = 'projects'
    resetDetailPages()
    await reloadDetails()
    return
  }
  refinements.value = {
    ...refinements.value,
    user_id: row.user_id,
    user_name: row.user_display_name
  }
  resetDetailPages()
  await reloadDetails()
}

async function selectModel(row: ModelUsageStatistics) {
  if (primaryDimension.value === 'model') {
    if (
      selectedModel.value?.model === row.model &&
      (selectedModel.value.channel_id ?? null) === (row.channel_id ?? null) &&
      selectedModel.value.billing_meter === row.billing_meter
    ) {
      await clearSelection()
      return
    }
    selectedModel.value = row
    selectedProject.value = null
    selectedUser.value = null
    refinements.value = {}
    detailTab.value = 'projects'
    resetDetailPages()
    await reloadDetails()
    return
  }
  refinements.value = {
    ...refinements.value,
    model: row.model,
    channel_id: row.channel_id ?? undefined,
    channel_name: row.channel_name,
    billing_meter: row.billing_meter
  }
  resetDetailPages()
  await reloadDetails()
}

async function refineProject(row: ProjectUsageStatistics) {
  if (row.project_id == null) return
  refinements.value = {
    ...refinements.value,
    project_id: row.project_id,
    project_name: row.project_name
  }
  resetDetailPages()
  await reloadDetails()
}

function clearRefinementKeys(keys: Array<keyof DrilldownContext>) {
  const next = { ...refinements.value }
  for (const key of keys) {
    delete next[key]
  }
  refinements.value = next
}

async function clearRefinement(kind?: keyof DrilldownContext) {
  if (!kind) {
    refinements.value = {}
  } else if (kind === 'model' || kind === 'channel_id' || kind === 'billing_meter') {
    clearRefinementKeys(['model', 'channel_id', 'channel_name', 'billing_meter'])
  } else if (kind === 'project_id') {
    clearRefinementKeys(['project_id', 'project_name'])
  } else if (kind === 'user_id') {
    clearRefinementKeys(['user_id', 'user_name'])
  }
  resetDetailPages()
  await reloadDetails()
}

async function clearSelection() {
  selectedProject.value = null
  selectedUser.value = null
  selectedModel.value = null
  refinements.value = {}
  resetDetailPages()
  detailProjects.value = emptyPage<ProjectUsageStatistics>()
  detailUsers.value = emptyPage<UserUsageStatistics | ProjectMemberUsageStatistics>()
  detailModels.value = emptyPage<ModelUsageStatistics>()
}

async function exportAttribution(scope: string | number | object) {
  if (typeof scope !== 'string') return
  const exportScope = exportScopeFromCommand(scope)
  if (!exportScope) return
  exporting.value = true
  try {
    const query = scope === 'primary' ? baseQuery.value : detailQuery.value
    const result = await downloadAdminUsageStatisticsCsv(exportScope, query)
    downloadBlob(result.filename ?? `usage-statistics-${exportScope}.csv`, result.blob)
  } finally {
    exporting.value = false
  }
}

function openUsageDetails(extra: DrilldownContext = {}) {
  const [start, end] = filters.dateRange ?? []
  const context = { ...selectedContext.value, ...extra }
  const route = router.resolve({
    path: '/admin/usage',
    query: compactQuery({
      start,
      end,
      project_id: context.project_id,
      user_id: context.user_id,
      channel_id: context.channel_id,
      model: context.model,
      billing_meter: context.billing_meter
    })
  })
  window.open(route.href, '_blank', 'noopener,noreferrer')
}

async function handlePrimaryPageChange(page: number) {
  primaryPage.value = page
  await reloadPrimary()
}

async function handlePrimaryPageSizeChange(size: number) {
  primaryPageSize.value = size
  primaryPage.value = 1
  await reloadPrimary()
}

async function handleDetailPageChange(tab: DetailTab, page: number) {
  detailPage(tab).value = page
  await reloadDetailTab(tab)
}

async function handleDetailPageSizeChange(tab: DetailTab, size: number) {
  detailPageSize(tab).value = size
  detailPage(tab).value = 1
  await reloadDetailTab(tab)
}

async function reloadDetailTab(tab: DetailTab) {
  if (tab === 'projects') await reloadDetailProjects()
  if (tab === 'users') await reloadDetailUsers()
  if (tab === 'models') await reloadDetailModels()
}

function detailPage(tab: DetailTab) {
  if (tab === 'projects') return detailProjectsPage
  if (tab === 'users') return detailUsersPage
  return detailModelsPage
}

function detailPageSize(tab: DetailTab) {
  if (tab === 'projects') return detailProjectsPageSize
  if (tab === 'users') return detailUsersPageSize
  return detailModelsPageSize
}

function detailPageData(tab: DetailTab) {
  if (tab === 'projects') return detailProjects.value
  if (tab === 'users') return detailUsers.value
  return detailModels.value
}

function defaultDetailTab(dimension: PrimaryDimension): DetailTab {
  if (dimension === 'project') return 'users'
  return 'projects'
}

function resetDetailPages() {
  detailProjectsPage.value = 1
  detailUsersPage.value = 1
  detailModelsPage.value = 1
}

function syncSelectedFromPrimary() {
  if (primaryDimension.value === 'project' && selectedProject.value?.project_id != null) {
    const next = primaryProjects.value.items.find((item) => item.project_id === selectedProject.value?.project_id)
    if (next) selectedProject.value = next
  }
  if (primaryDimension.value === 'user' && selectedUser.value?.user_id != null) {
    const next = primaryUsers.value.items.find((item) => item.user_id === selectedUser.value?.user_id)
    if (next) selectedUser.value = next
  }
  if (primaryDimension.value === 'model' && selectedModel.value) {
    const next = primaryModels.value.items.find(
      (item) =>
        item.model === selectedModel.value?.model &&
        item.channel_id === selectedModel.value?.channel_id &&
        item.billing_meter === selectedModel.value?.billing_meter
    )
    if (next) selectedModel.value = next
  }
}

function exportScopeFromCommand(command: string): UsageStatisticsExportScope | null {
  if (command === 'primary') {
    if (primaryDimension.value === 'project') return 'projects'
    if (primaryDimension.value === 'user') return 'users'
    return 'models'
  }
  if (command === 'detail') {
    if (detailTab.value === 'projects') return 'projects'
    if (detailTab.value === 'users') return detailQuery.value.project_id != null ? 'project_members' : 'users'
    return 'models'
  }
  return null
}

function compactQuery(values: Record<string, string | number | undefined>) {
  const query: Record<string, string> = {}
  for (const [key, value] of Object.entries(values)) {
    if (value != null && value !== '') query[key] = String(value)
  }
  return query
}

function defaultRange(days: number) {
  const end = new Date()
  const start = new Date()
  start.setDate(end.getDate() - (days - 1))
  return [toDateKey(start), toDateKey(end)]
}

function emptyPage<T>(): UsageStatisticsPage<T> {
  return { items: [], total: 0, page: 1, limit: DEFAULT_PAGE_SIZE }
}

function currentPrimaryPage() {
  if (primaryDimension.value === 'project') return primaryProjects.value
  if (primaryDimension.value === 'user') return primaryUsers.value
  return primaryModels.value
}

function billingMeterLabel(value?: string | null) {
  if (value === 'image') return t('billingMeterImageGeneration')
  if (value === 'video') return t('billingMeterVideo')
  if (value === 'token') return t('billingMeterToken')
  return t('billingMeterAll')
}

function projectRowKey(row: ProjectUsageStatistics) {
  return row.project_id ?? row.project_name
}

function userRowKey(row: UserUsageStatistics | ProjectMemberUsageStatistics) {
  return row.user_id ?? row.user_display_name
}

function modelRowKey(row: ModelUsageStatistics) {
  return `${row.channel_id ?? 'channel'}/${row.channel_name}/${row.model}/${row.billing_meter}`
}

function modelDisplay(row: Pick<ModelUsageStatistics, 'channel_name' | 'model'>) {
  return row.model ? `${row.channel_name || '-'}/${row.model}` : row.channel_name || '-'
}

function userDisplay(row: UserUsageStatistics | ProjectMemberUsageStatistics) {
  return row.user_display_name || row.user_email || row.user_username || '-'
}

function successRate(success: number, total: number) {
  if (total <= 0) return '-'
  return `${((success / total) * 100).toFixed(1)}%`
}
</script>

<template>
  <section class="grid usage-view cost-attribution-view">
    <div class="cost-attribution">
      <el-form class="usage-toolbar attribution-toolbar" @submit.prevent="reloadAttribution">
        <div class="usage-toolbar-filters">
          <label class="admin-filter-field">
            <span>{{ t('timeRange') }}</span>
            <el-date-picker
              v-model="filters.dateRange"
              class="usage-date-range"
              type="daterange"
              value-format="YYYY-MM-DD"
              :range-separator="t('to')"
              :start-placeholder="t('startTime')"
              :end-placeholder="t('endTime')"
            />
          </label>
          <label class="admin-filter-field">
            <span>{{ t('project') }}</span>
            <el-input
              v-model="filters.projectQuery"
              class="usage-search-input"
              clearable
              :prefix-icon="Search"
              :placeholder="t('costProjectSearchPlaceholder')"
            />
          </label>
          <label class="admin-filter-field">
            <span>{{ t('userSearch') }}</span>
            <el-input
              v-model="filters.userQuery"
              class="usage-search-input"
              clearable
              :prefix-icon="Search"
              :placeholder="t('userSearchPlaceholder')"
            />
          </label>
          <label class="admin-filter-field">
            <span>{{ t('model') }}</span>
            <el-select
              v-model="filters.model"
              class="usage-model-filter"
              clearable
              filterable
              :placeholder="t('allModels')"
            >
              <el-option
                v-for="item in filteredModelOptions"
                :key="`${item.channel_name}/${item.model}`"
                :label="modelDisplay(item)"
                :value="item.model"
              />
            </el-select>
          </label>
          <el-button class="admin-action-button" type="primary" native-type="submit" :icon="Search" :loading="loading">
            {{ t('search') }}
          </el-button>
        </div>
        <div class="usage-toolbar-actions attribution-toolbar-actions">
          <el-dropdown trigger="click" @command="exportAttribution">
            <el-button class="admin-action-button" :icon="Download" :loading="exporting">
              {{ t('exportAttribution') }}
            </el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="primary">{{ t('exportCurrentSummary') }}</el-dropdown-item>
                <el-dropdown-item command="detail" :disabled="!hasSelection">
                  {{ t('exportCurrentDrilldown') }}
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>
        <div class="attribution-quick-ranges">
          <el-button :class="{ 'is-active': activeQuickRange === 7 }" @click="applyQuickRange(7)">
            {{ t('quickRange7') }}
          </el-button>
          <el-button :class="{ 'is-active': activeQuickRange === 30 }" @click="applyQuickRange(30)">
            {{ t('quickRange30') }}
          </el-button>
          <el-button :class="{ 'is-active': activeQuickRange === 90 }" @click="applyQuickRange(90)">
            {{ t('quickRange90') }}
          </el-button>
        </div>
      </el-form>

      <section class="attribution-panel">
        <header class="attribution-panel-header">
          <span>{{ t('costAttributionDimension') }}</span>
          <small>{{ t('defaultSortByCost') }}</small>
        </header>
        <div class="attribution-tabs">
          <el-button :class="{ 'is-active': primaryDimension === 'project' }" @click="changePrimaryDimension('project')">
            {{ t('projectSummary') }}
          </el-button>
          <el-button :class="{ 'is-active': primaryDimension === 'user' }" @click="changePrimaryDimension('user')">
            {{ t('userSummary') }}
          </el-button>
          <el-button :class="{ 'is-active': primaryDimension === 'model' }" @click="changePrimaryDimension('model')">
            {{ t('modelSummary') }}
          </el-button>
        </div>

        <el-table
          v-if="primaryDimension === 'project'"
          v-loading="primaryProjectsLoading"
          class="admin-table service-table attribution-table"
          :data="primaryProjects.items"
          :row-key="projectRowKey"
          stripe
          highlight-current-row
          @row-click="selectProject"
        >
          <el-table-column :label="t('project')" min-width="190">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ row.project_name }}</strong>
                <span v-if="row.project_id != null">#{{ row.project_id }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="100" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('userCount')" min-width="100" align="right">
            <template #default="{ row }">{{ formatNumber(row.member_count, locale) }}</template>
          </el-table-column>
          <el-table-column min-width="90" align="right">
            <template #default="{ row }">
              <el-button class="icon-only-action" :aria-label="t('viewDrilldown')" :icon="ArrowRight" @click.stop="selectProject(row)" />
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>

        <el-table
          v-else-if="primaryDimension === 'user'"
          v-loading="primaryUsersLoading"
          class="admin-table service-table attribution-table"
          :data="primaryUsers.items"
          :row-key="userRowKey"
          stripe
          highlight-current-row
          @row-click="selectUser"
        >
          <el-table-column :label="t('usageUser')" min-width="190">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ userDisplay(row) }}</strong>
                <span v-if="row.user_id != null">#{{ row.user_id }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="100" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('modelCount')" min-width="100" align="right">
            <template #default="{ row }">{{ formatNumber(row.model_count, locale) }}</template>
          </el-table-column>
          <el-table-column min-width="90" align="right">
            <template #default="{ row }">
              <el-button class="icon-only-action" :aria-label="t('viewDrilldown')" :icon="ArrowRight" @click.stop="selectUser(row)" />
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>

        <el-table
          v-else
          v-loading="primaryModelsLoading"
          class="admin-table service-table attribution-table"
          :data="primaryModels.items"
          :row-key="modelRowKey"
          stripe
          highlight-current-row
          @row-click="selectModel"
        >
          <el-table-column :label="t('channelAndModel')" min-width="230">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ modelDisplay(row) }}</strong>
                <span v-if="row.channel_id != null">#{{ row.channel_id }} / {{ billingMeterLabel(row.billing_meter) }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="100" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('userCount')" min-width="100" align="right">
            <template #default="{ row }">{{ formatNumber(row.user_count, locale) }}</template>
          </el-table-column>
          <el-table-column min-width="90" align="right">
            <template #default="{ row }">
              <el-button class="icon-only-action" :aria-label="t('viewDrilldown')" :icon="ArrowRight" @click.stop="selectModel(row)" />
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>

        <div class="attribution-pagination">
          <el-pagination
            v-model:current-page="primaryPage"
            v-model:page-size="primaryPageSize"
            background
            layout="total, sizes, prev, pager, next"
            :total="currentPrimaryPage().total"
            :page-sizes="[20, 50, 100]"
            @current-change="handlePrimaryPageChange"
            @size-change="handlePrimaryPageSizeChange"
          />
        </div>
      </section>

      <section v-if="hasSelection" class="attribution-panel">
        <header class="attribution-panel-header">
          <span>{{ selectedAnalysisTitle }}</span>
          <div class="attribution-actions">
            <el-button class="admin-action-button" :icon="Tickets" @click="openUsageDetails()">
              {{ t('viewUsageDetails') }}
            </el-button>
            <el-button class="icon-only-action" :aria-label="t('clearSelection')" :icon="Close" @click="clearSelection" />
          </div>
        </header>

        <div class="attribution-context-bar">
          <span v-if="selectedContext.project_id != null" class="attribution-context-chip">
            {{ t('project') }}: {{ selectedContext.project_name || `#${selectedContext.project_id}` }}
            <button v-if="refinements.project_id != null" type="button" @click="clearRefinement('project_id')">x</button>
          </span>
          <span v-if="selectedContext.user_id != null" class="attribution-context-chip">
            {{ t('userSearch') }}: {{ selectedContext.user_name || `#${selectedContext.user_id}` }}
            <button v-if="refinements.user_id != null" type="button" @click="clearRefinement('user_id')">x</button>
          </span>
          <span v-if="selectedContext.model" class="attribution-context-chip">
            {{ t('model') }}: {{ selectedContext.channel_name || '-' }}/{{ selectedContext.model }}
            <button v-if="refinements.model" type="button" @click="clearRefinement('model')">x</button>
          </span>
          <el-button v-if="Object.keys(refinements).length" class="attribution-clear-context" text @click="clearRefinement()">
            {{ t('clearDrilldownFilters') }}
          </el-button>
        </div>

        <div class="attribution-tabs">
          <el-button
            v-for="tab in detailTabs"
            :key="tab"
            :class="{ 'is-active': detailTab === tab }"
            @click="detailTab = tab"
          >
            {{ t(`drilldown_${tab}`) }}
          </el-button>
        </div>

        <el-table
          v-if="detailTab === 'projects'"
          v-loading="detailProjectsLoading"
          class="admin-table service-table attribution-table"
          :data="detailProjects.items"
          :row-key="projectRowKey"
          stripe
          @row-click="refineProject"
        >
          <el-table-column :label="t('project')" min-width="190">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ row.project_name }}</strong>
                <span v-if="row.project_id != null">#{{ row.project_id }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="100" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column min-width="130" align="right">
            <template #default="{ row }">
              <el-button class="admin-action-button" :icon="Tickets" @click.stop="openUsageDetails({ project_id: row.project_id, project_name: row.project_name })">
                {{ t('details') }}
              </el-button>
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>

        <el-table
          v-else-if="detailTab === 'users'"
          v-loading="detailUsersLoading"
          class="admin-table service-table attribution-table"
          :data="detailUsers.items"
          :row-key="userRowKey"
          stripe
          @row-click="selectUser"
        >
          <el-table-column :label="t('usageUser')" min-width="190">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ userDisplay(row) }}</strong>
                <span v-if="row.user_id != null">#{{ row.user_id }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="100" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column min-width="130" align="right">
            <template #default="{ row }">
              <el-button class="admin-action-button" :icon="Tickets" @click.stop="openUsageDetails({ user_id: row.user_id, user_name: row.user_display_name })">
                {{ t('details') }}
              </el-button>
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>

        <el-table
          v-else
          v-loading="detailModelsLoading"
          class="admin-table service-table attribution-table"
          :data="detailModels.items"
          :row-key="modelRowKey"
          stripe
          @row-click="selectModel"
        >
          <el-table-column :label="t('channelAndModel')" min-width="230">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ modelDisplay(row) }}</strong>
                <span v-if="row.channel_id != null">#{{ row.channel_id }} / {{ billingMeterLabel(row.billing_meter) }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="100" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column min-width="130" align="right">
            <template #default="{ row }">
              <el-button
                class="admin-action-button"
                :icon="Tickets"
                @click.stop="openUsageDetails({ model: row.model, channel_id: row.channel_id, channel_name: row.channel_name, billing_meter: row.billing_meter })"
              >
                {{ t('details') }}
              </el-button>
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>

        <div class="attribution-pagination">
          <el-pagination
            v-model:current-page="activeDetailPage"
            v-model:page-size="activeDetailPageSize"
            background
            layout="total, sizes, prev, pager, next"
            :total="detailPageData(detailTab).total"
            :page-sizes="[20, 50, 100]"
            @current-change="(page: number) => handleDetailPageChange(detailTab, page)"
            @size-change="(size: number) => handleDetailPageSizeChange(detailTab, size)"
          />
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.cost-attribution {
  display: grid;
  gap: 18px;
  min-width: 0;
}

.attribution-toolbar,
.attribution-panel {
  background: #ffffff;
  border: 1px solid #dfe8f2;
  border-radius: 8px;
  box-shadow: 0 10px 30px rgba(15, 23, 42, 0.04);
}

.attribution-toolbar {
  align-items: center;
  display: grid;
  gap: 10px 12px;
  grid-template-columns: minmax(0, 1fr) auto;
  padding: 14px 16px;
}

.attribution-toolbar .usage-toolbar-filters {
  flex-wrap: nowrap;
  row-gap: 10px;
}

.attribution-toolbar .admin-filter-field {
  flex: 0 1 auto;
  gap: 6px;
}

.attribution-toolbar .admin-filter-field > span {
  color: #667085;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 680;
  white-space: nowrap;
}

.attribution-toolbar :deep(.usage-date-range.el-date-editor.el-input__wrapper) {
  --el-date-editor-width: 240px;
  flex: 0 0 240px;
  max-width: 240px;
  width: 240px;
}

.attribution-toolbar .usage-search-input.el-input {
  flex-basis: 180px;
  width: 180px;
}

.attribution-toolbar .usage-model-filter {
  flex-basis: 180px;
  width: 180px;
}

.attribution-toolbar-actions {
  align-self: start;
  flex-wrap: nowrap;
  justify-content: flex-end;
}

.attribution-quick-ranges,
.attribution-tabs,
.attribution-context-bar {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.attribution-quick-ranges {
  gap: 4px;
  grid-column: 1 / -1;
  padding-top: 2px;
}

.attribution-quick-ranges .el-button {
  --el-button-bg-color: transparent;
  --el-button-border-color: transparent;
  --el-button-hover-bg-color: #f2f6fb;
  --el-button-hover-border-color: transparent;
  --el-button-hover-text-color: #475467;
  --el-button-text-color: #667085;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 620;
  height: 30px;
  min-height: 30px;
  padding: 0 8px;
}

.attribution-quick-ranges .el-button + .el-button {
  margin-left: 0;
}

.attribution-quick-ranges .el-button.is-active {
  --el-button-bg-color: #eef7ff;
  --el-button-border-color: #b7d8f3;
  --el-button-text-color: #168bd3;
}

.attribution-tabs .el-button {
  --el-button-bg-color: transparent;
  --el-button-border-color: transparent;
  --el-button-hover-bg-color: #f2f6fb;
  --el-button-hover-border-color: transparent;
  --el-button-hover-text-color: #475467;
  --el-button-text-color: #667085;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 650;
  height: 30px;
  min-height: 30px;
  padding: 0 9px;
}

.attribution-tabs .el-button + .el-button {
  margin-left: 0;
}

.attribution-tabs .el-button.is-active {
  --el-button-bg-color: #eef7ff;
  --el-button-border-color: #b7d8f3;
  --el-button-text-color: #168bd3;
}

.attribution-panel {
  display: grid;
  gap: 16px;
  min-width: 0;
  padding: 18px;
}

.attribution-panel-header {
  align-items: center;
  color: #1d2939;
  display: grid;
  font-size: 15px;
  font-weight: 720;
  gap: 8px;
  grid-template-columns: auto minmax(0, 1fr) auto;
}

.attribution-panel-header small {
  color: #98a2b3;
  font-size: 12px;
  font-weight: 560;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-actions {
  align-items: center;
  display: flex;
  gap: 6px;
  justify-content: flex-end;
}

.attribution-context-bar {
  padding: 0;
}

.attribution-context-chip {
  align-items: center;
  background: #ffffff;
  border: 1px solid #dfe8f2;
  border-radius: 999px;
  color: #475467;
  display: inline-flex;
  font-size: 12px;
  font-weight: 620;
  gap: 6px;
  max-width: 280px;
  min-height: 28px;
  overflow: hidden;
  padding: 0 9px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-context-chip button {
  background: transparent;
  border: 0;
  color: #98a2b3;
  cursor: pointer;
  font: inherit;
  padding: 0;
}

.attribution-clear-context {
  font-size: 12px;
}

.attribution-table {
  width: 100%;
}

.attribution-primary-cell {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.attribution-primary-cell strong {
  color: #1d2939;
  font-size: 13px;
  font-weight: 680;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-primary-cell span {
  color: #98a2b3;
  font-size: 12px;
  font-weight: 560;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-pagination {
  display: flex;
  justify-content: flex-end;
  padding-top: 4px;
}

@media (max-width: 1180px) {
  .attribution-toolbar {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .attribution-toolbar .usage-toolbar-filters {
    flex-wrap: wrap;
  }

  .attribution-toolbar-actions,
  .attribution-pagination {
    justify-content: flex-start;
  }

  .attribution-panel-header {
    grid-template-columns: minmax(0, 1fr);
  }

  .attribution-actions {
    justify-content: flex-start;
  }
}

@media (max-width: 760px) {
  .attribution-toolbar,
  .attribution-panel {
    padding: 14px;
  }

  .attribution-toolbar .usage-toolbar-filters,
  .attribution-toolbar .usage-toolbar-actions {
    display: grid;
    grid-template-columns: 1fr;
  }

  .attribution-toolbar .admin-filter-field {
    align-items: stretch;
    display: grid;
    gap: 5px;
  }
}
</style>
