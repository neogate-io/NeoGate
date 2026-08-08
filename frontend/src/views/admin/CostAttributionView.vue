<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Close, Download, Search, Tickets } from '@element-plus/icons-vue'
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
import AttributionDimensionTable, {
  type AttributionDimension,
  type AttributionRow
} from '../../components/admin/usage/AttributionDimensionTable.vue'
import { useAsyncData } from '../../composables/useAsyncData'
import { useDownloadTask } from '../../composables/useDownloadTask'
import { useLocale } from '../../composables/useLocale'
import { downloadBlob, toDateKey } from '../../utils/format'

const { t } = useLocale()
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
  billing_meter?: 'token' | 'image' | 'video' | 'audio'
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
const { downloading: exporting, run: runDownload } = useDownloadTask()

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
const detailDimension = computed<AttributionDimension>(() => {
  if (detailTab.value === 'projects') return 'project'
  if (detailTab.value === 'users') return 'user'
  return 'model'
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

const primaryData = useAsyncData<UsageStatisticsPage<AttributionRow>>(
  () =>
    loadDimensionPage(
      primaryDimension.value,
      baseQuery.value,
      primaryPage.value,
      primaryPageSize.value
    ),
  emptyPage<AttributionRow>()
)
const detailData = useAsyncData<UsageStatisticsPage<AttributionRow>>(() => {
  if (!hasSelection.value) return Promise.resolve(emptyPage<AttributionRow>())
  return loadDetailPage(
    detailTab.value,
    detailQuery.value,
    activeDetailPage.value,
    activeDetailPageSize.value
  )
}, emptyPage<AttributionRow>())

const primaryProjects = computed(() => pageAs<ProjectUsageStatistics>(primaryData.data.value))
const primaryUsers = computed(() => pageAs<UserUsageStatistics>(primaryData.data.value))
const primaryModels = computed(() => pageAs<ModelUsageStatistics>(primaryData.data.value))
const detailProjects = computed(() => pageAs<ProjectUsageStatistics>(detailData.data.value))
const detailUsers = computed(() =>
  pageAs<UserUsageStatistics | ProjectMemberUsageStatistics>(detailData.data.value)
)
const detailModels = computed(() => pageAs<ModelUsageStatistics>(detailData.data.value))
const {
  data: attributionOptions,
  loading: attributionOptionsLoading,
  reload: reloadAttributionOptions
} = useAsyncData(() => getAdminUsageStatisticsOptions(baseQuery.value), { models: [], users: [] })

const primaryLoading = primaryData.loading
const detailLoading = detailData.loading
const loading = computed(
  () => primaryLoading.value || detailLoading.value || attributionOptionsLoading.value
)
const filteredModelOptions = computed(() => attributionOptions.value.models)

async function reloadPrimary() {
  await primaryData.reload()
  syncSelectedFromPrimary()
}

async function reloadDetails() {
  if (!hasSelection.value) return
  await detailData.reload()
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

async function changeDetailTab(tab: DetailTab) {
  if (detailTab.value === tab) return
  detailTab.value = tab
  detailPage(tab).value = 1
  await reloadDetails()
}

function selectPrimaryRow(row: AttributionRow) {
  if (primaryDimension.value === 'project') return selectProject(row as ProjectUsageStatistics)
  if (primaryDimension.value === 'user') return selectUser(row as UserUsageStatistics)
  return selectModel(row as ModelUsageStatistics)
}

function selectDetailRow(row: AttributionRow) {
  if (detailTab.value === 'projects') return refineProject(row as ProjectUsageStatistics)
  if (detailTab.value === 'users') {
    return selectUser(row as UserUsageStatistics | ProjectMemberUsageStatistics)
  }
  return selectModel(row as ModelUsageStatistics)
}

function openDetailRow(row: AttributionRow) {
  if (detailTab.value === 'projects') {
    const item = row as ProjectUsageStatistics
    return openUsageDetails({
      project_id: item.project_id ?? undefined,
      project_name: item.project_name
    })
  }
  if (detailTab.value === 'users') {
    const item = row as UserUsageStatistics | ProjectMemberUsageStatistics
    return openUsageDetails({
      user_id: item.user_id ?? undefined,
      user_name: item.user_display_name
    })
  }
  const item = row as ModelUsageStatistics
  return openUsageDetails({
    model: item.model,
    channel_id: item.channel_id ?? undefined,
    channel_name: item.channel_name,
    billing_meter: item.billing_meter
  })
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
  detailData.data.value = emptyPage<AttributionRow>()
}

async function exportAttribution(scope: string | number | object) {
  if (typeof scope !== 'string') return
  const exportScope = exportScopeFromCommand(scope)
  if (!exportScope) return
  await runDownload(async () => {
    const query = scope === 'primary' ? baseQuery.value : detailQuery.value
    const result = await downloadAdminUsageStatisticsCsv(exportScope, query)
    downloadBlob(result.filename ?? `usage-statistics-${exportScope}.csv`, result.blob)
  })
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
  if (detailTab.value !== tab) detailTab.value = tab
  await reloadDetails()
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
    const next = primaryProjects.value.items.find(
      (item) => item.project_id === selectedProject.value?.project_id
    )
    if (next) selectedProject.value = next
  }
  if (primaryDimension.value === 'user' && selectedUser.value?.user_id != null) {
    const next = primaryUsers.value.items.find(
      (item) => item.user_id === selectedUser.value?.user_id
    )
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
    if (detailTab.value === 'users')
      return detailQuery.value.project_id != null ? 'project_members' : 'users'
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

async function loadDimensionPage(
  dimension: PrimaryDimension,
  query: UsageStatisticsQuery,
  page: number,
  limit: number
): Promise<UsageStatisticsPage<AttributionRow>> {
  const request = { ...query, page, limit }
  if (dimension === 'project') return getAdminUsageStatisticsProjects(request)
  if (dimension === 'user') return getAdminUsageStatisticsUsers(request)
  return getAdminUsageStatisticsModels(request)
}

async function loadDetailPage(
  tab: DetailTab,
  query: UsageStatisticsQuery,
  page: number,
  limit: number
): Promise<UsageStatisticsPage<AttributionRow>> {
  const request = { ...query, page, limit }
  if (tab === 'projects') return getAdminUsageStatisticsProjects(request)
  if (tab === 'users' && query.project_id != null) {
    return getAdminUsageStatisticsProjectMembers(request)
  }
  if (tab === 'users') return getAdminUsageStatisticsUsers(request)
  return getAdminUsageStatisticsModels(request)
}

function pageAs<T>(page: UsageStatisticsPage<AttributionRow>): UsageStatisticsPage<T> {
  return page as UsageStatisticsPage<T>
}

function modelDisplay(row: Pick<ModelUsageStatistics, 'channel_name' | 'model'>) {
  return row.model ? `${row.channel_name || '-'}/${row.model}` : row.channel_name || '-'
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
          <el-button
            class="admin-action-button"
            type="primary"
            native-type="submit"
            :icon="Search"
            :loading="loading"
          >
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
                <el-dropdown-item command="primary">{{
                  t('exportCurrentSummary')
                }}</el-dropdown-item>
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
          <el-button
            :class="{ 'is-active': primaryDimension === 'project' }"
            @click="changePrimaryDimension('project')"
          >
            {{ t('projectSummary') }}
          </el-button>
          <el-button
            :class="{ 'is-active': primaryDimension === 'user' }"
            @click="changePrimaryDimension('user')"
          >
            {{ t('userSummary') }}
          </el-button>
          <el-button
            :class="{ 'is-active': primaryDimension === 'model' }"
            @click="changePrimaryDimension('model')"
          >
            {{ t('modelSummary') }}
          </el-button>
        </div>

        <AttributionDimensionTable
          :kind="primaryDimension"
          :rows="currentPrimaryPage().items"
          :loading="primaryLoading"
          primary
          @select="selectPrimaryRow"
        />

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
            <el-button
              class="icon-only-action"
              :aria-label="t('clearSelection')"
              :icon="Close"
              @click="clearSelection"
            />
          </div>
        </header>

        <div class="attribution-context-bar">
          <span v-if="selectedContext.project_id != null" class="attribution-context-chip">
            {{ t('project') }}:
            {{ selectedContext.project_name || `#${selectedContext.project_id}` }}
            <button
              v-if="refinements.project_id != null"
              type="button"
              @click="clearRefinement('project_id')"
            >
              x
            </button>
          </span>
          <span v-if="selectedContext.user_id != null" class="attribution-context-chip">
            {{ t('userSearch') }}: {{ selectedContext.user_name || `#${selectedContext.user_id}` }}
            <button
              v-if="refinements.user_id != null"
              type="button"
              @click="clearRefinement('user_id')"
            >
              x
            </button>
          </span>
          <span v-if="selectedContext.model" class="attribution-context-chip">
            {{ t('model') }}: {{ selectedContext.channel_name || '-' }}/{{ selectedContext.model }}
            <button v-if="refinements.model" type="button" @click="clearRefinement('model')">
              x
            </button>
          </span>
          <el-button
            v-if="Object.keys(refinements).length"
            class="attribution-clear-context"
            text
            @click="clearRefinement()"
          >
            {{ t('clearDrilldownFilters') }}
          </el-button>
        </div>

        <div class="attribution-tabs">
          <el-button
            v-for="tab in detailTabs"
            :key="tab"
            :class="{ 'is-active': detailTab === tab }"
            @click="changeDetailTab(tab)"
          >
            {{ t(`drilldown_${tab}`) }}
          </el-button>
        </div>

        <AttributionDimensionTable
          :kind="detailDimension"
          :rows="detailPageData(detailTab).items"
          :loading="detailLoading"
          @select="selectDetailRow"
          @details="openDetailRow"
        />

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
