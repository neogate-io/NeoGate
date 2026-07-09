<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { ArrowRight, Close, Download, Search } from '@element-plus/icons-vue'
import {
  downloadAdminUsageStatisticsCsv,
  getAdminUsageStatisticsKeys,
  getAdminUsageStatisticsModels,
  getAdminUsageStatisticsProjectMembers,
  getAdminUsageStatisticsProjects,
  getAdminUsageStatisticsUsers,
  type KeyUsageStatistics,
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
import { downloadBlob, formatDurationMs, formatNumber, toDateKey } from '../../utils/format'

const { locale, t } = useLocale()
const { formatMoney } = useBillingCurrency()

type AttributionFilters = {
  dateRange: string[] | null
  projectQuery: string
  userQuery: string
}

const filters = reactive<AttributionFilters>({
  dateRange: defaultRange(30),
  projectQuery: '',
  userQuery: ''
})
const projectsPage = ref(1)
const projectsPageSize = ref(20)
const membersPage = ref(1)
const membersPageSize = ref(20)
const keysPage = ref(1)
const keysPageSize = ref(20)
const usersPage = ref(1)
const usersPageSize = ref(20)
const modelsPage = ref(1)
const modelsPageSize = ref(20)
const selectedProject = ref<ProjectUsageStatistics | null>(null)
const selectedMember = ref<ProjectMemberUsageStatistics | null>(null)
const exporting = ref(false)

const baseQuery = computed<UsageStatisticsQuery>(() => {
  const [start, end] = filters.dateRange ?? []
  return {
    start,
    end,
    project_query: filters.projectQuery.trim() || undefined,
    user_query: filters.userQuery.trim() || undefined,
    sort: 'cost_desc'
  }
})

const {
  data: projects,
  loading: projectsLoading,
  reload: reloadProjects
} = useAsyncData(
  () =>
    getAdminUsageStatisticsProjects({
      ...baseQuery.value,
      page: projectsPage.value,
      limit: projectsPageSize.value
    }),
  emptyPage<ProjectUsageStatistics>()
)

const {
  data: members,
  loading: membersLoading,
  reload: reloadMembers
} = useAsyncData(
  () => {
    if (selectedProject.value?.project_id == null) {
      return Promise.resolve(emptyPage<ProjectMemberUsageStatistics>())
    }
    return getAdminUsageStatisticsProjectMembers({
      ...baseQuery.value,
      project_id: selectedProject.value.project_id,
      page: membersPage.value,
      limit: membersPageSize.value
    })
  },
  emptyPage<ProjectMemberUsageStatistics>()
)

const {
  data: keys,
  loading: keysLoading,
  reload: reloadKeys
} = useAsyncData(
  () => {
    if (selectedProject.value?.project_id == null) {
      return Promise.resolve(emptyPage<KeyUsageStatistics>())
    }
    return getAdminUsageStatisticsKeys({
      ...baseQuery.value,
      project_id: selectedProject.value.project_id,
      user_id: selectedMember.value?.user_id ?? undefined,
      page: keysPage.value,
      limit: keysPageSize.value
    })
  },
  emptyPage<KeyUsageStatistics>()
)

const {
  data: users,
  loading: usersLoading,
  reload: reloadUsers
} = useAsyncData(
  () =>
    getAdminUsageStatisticsUsers({
      ...baseQuery.value,
      page: usersPage.value,
      limit: usersPageSize.value
    }),
  emptyPage<UserUsageStatistics>()
)

const {
  data: models,
  loading: modelsLoading,
  reload: reloadModels
} = useAsyncData(
  () =>
    getAdminUsageStatisticsModels({
      ...baseQuery.value,
      page: modelsPage.value,
      limit: modelsPageSize.value
    }),
  emptyPage<ModelUsageStatistics>()
)

const loading = computed(
  () =>
    projectsLoading.value ||
    membersLoading.value ||
    keysLoading.value ||
    usersLoading.value ||
    modelsLoading.value
)
const activeQuickRange = computed(() => {
  for (const days of [7, 30, 90]) {
    const [start, end] = defaultRange(days)
    if (filters.dateRange?.[0] === start && filters.dateRange?.[1] === end) return days
  }
  return null
})

async function reloadAttribution() {
  projectsPage.value = 1
  membersPage.value = 1
  keysPage.value = 1
  usersPage.value = 1
  modelsPage.value = 1
  await Promise.all([reloadProjects(), reloadUsers(), reloadModels()])
  if (selectedProject.value) {
    await Promise.all([reloadMembers(), reloadKeys()])
  }
}

async function applyQuickRange(days: number) {
  filters.dateRange = defaultRange(days)
  await reloadAttribution()
}

async function selectProject(row: ProjectUsageStatistics) {
  if (row.project_id == null) return
  selectedProject.value = row
  selectedMember.value = null
  membersPage.value = 1
  keysPage.value = 1
  await Promise.all([reloadMembers(), reloadKeys()])
}

async function selectMember(row: ProjectMemberUsageStatistics) {
  selectedMember.value = row
  keysPage.value = 1
  await reloadKeys()
}

async function clearSelectedMember() {
  selectedMember.value = null
  keysPage.value = 1
  await reloadKeys()
}

function clearProjectSelection(clearTables = true) {
  selectedProject.value = null
  selectedMember.value = null
  membersPage.value = 1
  keysPage.value = 1
  if (clearTables) {
    members.value = emptyPage<ProjectMemberUsageStatistics>()
    keys.value = emptyPage<KeyUsageStatistics>()
  }
}

async function handleProjectsPageChange(page: number) {
  projectsPage.value = page
  await reloadProjects()
}

async function handleProjectsPageSizeChange(size: number) {
  projectsPageSize.value = size
  projectsPage.value = 1
  await reloadProjects()
}

async function handleMembersPageChange(page: number) {
  membersPage.value = page
  await reloadMembers()
}

async function handleMembersPageSizeChange(size: number) {
  membersPageSize.value = size
  membersPage.value = 1
  await reloadMembers()
}

async function handleKeysPageChange(page: number) {
  keysPage.value = page
  await reloadKeys()
}

async function handleKeysPageSizeChange(size: number) {
  keysPageSize.value = size
  keysPage.value = 1
  await reloadKeys()
}

async function handleUsersPageChange(page: number) {
  usersPage.value = page
  await reloadUsers()
}

async function handleUsersPageSizeChange(size: number) {
  usersPageSize.value = size
  usersPage.value = 1
  await reloadUsers()
}

async function handleModelsPageChange(page: number) {
  modelsPage.value = page
  await reloadModels()
}

async function handleModelsPageSizeChange(size: number) {
  modelsPageSize.value = size
  modelsPage.value = 1
  await reloadModels()
}

async function exportAttribution(scope: string | number | object) {
  if (typeof scope !== 'string') return
  const exportScope = scope as UsageStatisticsExportScope
  const query = attributionExportQuery(exportScope)
  if (!query) return
  exporting.value = true
  try {
    const result = await downloadAdminUsageStatisticsCsv(exportScope, query)
    downloadBlob(result.filename ?? `usage-statistics-${scope}.csv`, result.blob)
  } finally {
    exporting.value = false
  }
}

function attributionExportQuery(scope: UsageStatisticsExportScope): UsageStatisticsQuery | null {
  if (scope === 'project_members') {
    if (selectedProject.value?.project_id == null) return null
    return {
      ...baseQuery.value,
      project_id: selectedProject.value.project_id
    }
  }
  if (scope === 'keys') {
    if (selectedProject.value?.project_id == null) return null
    return {
      ...baseQuery.value,
      project_id: selectedProject.value.project_id,
      user_id: selectedMember.value?.user_id ?? undefined
    }
  }
  return baseQuery.value
}

function defaultRange(days: number) {
  const end = new Date()
  const start = new Date()
  start.setDate(end.getDate() - (days - 1))
  return [toDateKey(start), toDateKey(end)]
}

function emptyPage<T>(): UsageStatisticsPage<T> {
  return { items: [], total: 0, page: 1, limit: 20 }
}

function billingMeterLabel(value?: string | null) {
  if (value === 'image') return t('billingMeterImageGeneration')
  if (value === 'token') return t('billingMeterToken')
  return t('billingMeterAll')
}

function projectRowKey(row: ProjectUsageStatistics) {
  return row.project_id ?? row.project_name
}

function memberRowKey(row: ProjectMemberUsageStatistics) {
  return `${row.project_id ?? 'project'}/${row.user_id ?? row.user_display_name}`
}

function keyRowKey(row: KeyUsageStatistics) {
  return row.user_key_id ?? `${row.project_id ?? 'project'}/${row.user_key_name}`
}

function modelRowKey(row: ModelUsageStatistics) {
  return `${row.channel_name}/${row.model}/${row.billing_meter}`
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
          <el-button class="admin-action-button" type="primary" native-type="submit" :icon="Search" :loading="loading">
            {{ t('search') }}
          </el-button>
        </div>
        <div class="attribution-toolbar-actions">
          <el-dropdown trigger="click" @command="exportAttribution">
            <el-button
              class="admin-action-button"
              :icon="Download"
              :loading="exporting"
            >
              {{ t('exportAttribution') }}
            </el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="projects">{{ t('exportProjectSummary') }}</el-dropdown-item>
                <el-dropdown-item
                  command="project_members"
                  :disabled="selectedProject?.project_id == null"
                >
                  {{ t('exportMemberRanking') }}
                </el-dropdown-item>
                <el-dropdown-item command="keys" :disabled="selectedProject?.project_id == null">
                  {{ t('exportKeyDetails') }}
                </el-dropdown-item>
                <el-dropdown-item command="users">{{ t('exportUserSummary') }}</el-dropdown-item>
                <el-dropdown-item command="models">{{ t('exportModelSummary') }}</el-dropdown-item>
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
          <span>{{ t('projectSummary') }}</span>
          <small>{{ t('defaultSortByCost') }}</small>
        </header>
        <el-table
          v-loading="projectsLoading"
          class="admin-table service-table attribution-table"
          :data="projects.items"
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
          <el-table-column :label="t('memberCount')" min-width="100" align="right">
            <template #default="{ row }">{{ formatNumber(row.member_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('keyCount')" min-width="90" align="right">
            <template #default="{ row }">{{ formatNumber(row.key_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="110" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale, 6) }}</template>
          </el-table-column>
          <el-table-column min-width="90" align="right">
            <template #default="{ row }">
              <el-button
                class="icon-only-action"
                :aria-label="t('viewProjectMembers')"
                :icon="ArrowRight"
                @click.stop="selectProject(row)"
              />
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>
        <div class="attribution-pagination">
          <el-pagination
            v-model:current-page="projectsPage"
            v-model:page-size="projectsPageSize"
            background
            layout="total, sizes, prev, pager, next"
            :total="projects.total"
            :page-sizes="[20, 50, 100]"
            @current-change="handleProjectsPageChange"
            @size-change="handleProjectsPageSizeChange"
          />
        </div>
      </section>

      <section v-if="selectedProject" class="attribution-panel">
        <header class="attribution-panel-header">
          <span>{{ t('projectCostAttribution') }}</span>
          <small>
            {{ t('selectedProject') }}: {{ selectedProject.project_name }}
            <template v-if="selectedMember">
              / {{ t('selectedMember') }}: {{ selectedMember.user_display_name }}
            </template>
          </small>
          <div class="attribution-actions">
            <el-button
              v-if="selectedMember"
              class="icon-only-action"
              :aria-label="t('clearMemberSelection')"
              :icon="Close"
              @click="clearSelectedMember"
            />
            <el-button
              class="icon-only-action"
              :aria-label="t('clearProjectSelection')"
              :icon="Close"
              @click="clearProjectSelection()"
            />
          </div>
        </header>

        <div class="attribution-breakdown">
          <div>
            <span>{{ t('chatCost') }}</span>
            <strong>{{ formatMoney(selectedProject.cost_breakdown.chat_cost_micros, locale, 6) }}</strong>
          </div>
          <div>
            <span>{{ t('imageCost') }}</span>
            <strong>{{ formatMoney(selectedProject.cost_breakdown.image_cost_micros, locale, 6) }}</strong>
          </div>
          <div>
            <span>{{ t('codingCost') }}</span>
            <strong>{{ formatMoney(selectedProject.cost_breakdown.coding_cost_micros, locale, 6) }}</strong>
          </div>
          <div>
            <span>{{ t('otherCost') }}</span>
            <strong>{{ formatMoney(selectedProject.cost_breakdown.other_cost_micros, locale, 6) }}</strong>
          </div>
        </div>

        <div class="attribution-subtable">
          <div class="attribution-subheader">
            <strong>{{ t('memberRanking') }}</strong>
            <span>{{ selectedProject.project_name }}</span>
          </div>
          <el-table
            v-loading="membersLoading"
            class="admin-table service-table attribution-table"
            :data="members.items"
            :row-key="memberRowKey"
            stripe
            highlight-current-row
            @row-click="selectMember"
          >
            <el-table-column :label="t('projectMember')" min-width="190">
              <template #default="{ row }">
                <div class="attribution-primary-cell">
                  <strong>{{ row.user_display_name }}</strong>
                  <span v-if="row.user_id != null">#{{ row.user_id }}</span>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('keyCount')" min-width="90" align="right">
              <template #default="{ row }">{{ formatNumber(row.key_count, locale) }}</template>
            </el-table-column>
            <el-table-column :label="t('tokens')" min-width="110" align="right">
              <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
            </el-table-column>
            <el-table-column :label="t('cost')" min-width="120" align="right">
              <template #default="{ row }">{{ formatMoney(row.cost_micros, locale, 6) }}</template>
            </el-table-column>
            <el-table-column min-width="90" align="right">
              <template #default="{ row }">
                <el-button
                  class="icon-only-action"
                  :aria-label="t('viewKeys')"
                  :icon="ArrowRight"
                  @click.stop="selectMember(row)"
                />
              </template>
            </el-table-column>
            <template #empty>
              <el-empty :description="t('noStatisticsData')" />
            </template>
          </el-table>
          <div class="attribution-pagination">
            <el-pagination
              v-model:current-page="membersPage"
              v-model:page-size="membersPageSize"
              background
              layout="total, sizes, prev, pager, next"
              :total="members.total"
              :page-sizes="[20, 50, 100]"
              @current-change="handleMembersPageChange"
              @size-change="handleMembersPageSizeChange"
            />
          </div>
        </div>

        <div class="attribution-subtable">
          <div class="attribution-subheader">
            <strong>{{ t('keyDetails') }}</strong>
            <span>{{ selectedMember ? selectedMember.user_display_name : selectedProject.project_name }}</span>
          </div>
          <el-table
            v-loading="keysLoading"
            class="admin-table service-table attribution-table"
            :data="keys.items"
            :row-key="keyRowKey"
            stripe
          >
            <el-table-column :label="t('apiKey')" min-width="190">
              <template #default="{ row }">
                <div class="attribution-primary-cell">
                  <strong>{{ row.user_key_name }}</strong>
                  <span>{{ row.key_prefix || '-' }}</span>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('usageUser')" min-width="170">
              <template #default="{ row }">{{ row.user_display_name }}</template>
            </el-table-column>
            <el-table-column :label="t('requestCount')" min-width="110" align="right">
              <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
            </el-table-column>
            <el-table-column :label="t('tokens')" min-width="110" align="right">
              <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
            </el-table-column>
            <el-table-column :label="t('chatCost')" min-width="115" align="right">
              <template #default="{ row }">{{ formatMoney(row.cost_breakdown.chat_cost_micros, locale, 6) }}</template>
            </el-table-column>
            <el-table-column :label="t('imageCost')" min-width="115" align="right">
              <template #default="{ row }">{{ formatMoney(row.cost_breakdown.image_cost_micros, locale, 6) }}</template>
            </el-table-column>
            <el-table-column :label="t('codingCost')" min-width="115" align="right">
              <template #default="{ row }">{{ formatMoney(row.cost_breakdown.coding_cost_micros, locale, 6) }}</template>
            </el-table-column>
            <el-table-column :label="t('cost')" min-width="120" align="right">
              <template #default="{ row }">{{ formatMoney(row.cost_micros, locale, 6) }}</template>
            </el-table-column>
            <template #empty>
              <el-empty :description="t('noStatisticsData')" />
            </template>
          </el-table>
          <div class="attribution-pagination">
            <el-pagination
              v-model:current-page="keysPage"
              v-model:page-size="keysPageSize"
              background
              layout="total, sizes, prev, pager, next"
              :total="keys.total"
              :page-sizes="[20, 50, 100]"
              @current-change="handleKeysPageChange"
              @size-change="handleKeysPageSizeChange"
            />
          </div>
        </div>
      </section>

      <section class="attribution-panel">
        <header class="attribution-panel-header">
          <span>{{ t('userSummary') }}</span>
          <small>{{ t('defaultSortByCost') }}</small>
        </header>
        <el-table
          v-loading="usersLoading"
          class="admin-table service-table attribution-table"
          :data="users.items"
          row-key="user_id"
          stripe
        >
          <el-table-column :label="t('usageUser')" min-width="190">
            <template #default="{ row }">
              <div class="attribution-primary-cell">
                <strong>{{ row.user_display_name }}</strong>
                <span v-if="row.user_id != null">#{{ row.user_id }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('successRate')" min-width="110" align="right">
            <template #default="{ row }">{{ successRate(row.success_count, row.request_count) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="130" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('billingUnits')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.billable_units, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale, 6) }}</template>
          </el-table-column>
          <el-table-column :label="t('averageLatencyShort')" min-width="120" align="right">
            <template #default="{ row }">{{ formatDurationMs(row.avg_latency_ms) }}</template>
          </el-table-column>
          <el-table-column :label="t('modelCount')" min-width="100" align="right">
            <template #default="{ row }">{{ formatNumber(row.model_count, locale) }}</template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>
        <div class="attribution-pagination">
          <el-pagination
            v-model:current-page="usersPage"
            v-model:page-size="usersPageSize"
            background
            layout="total, sizes, prev, pager, next"
            :total="users.total"
            :page-sizes="[20, 50, 100]"
            @current-change="handleUsersPageChange"
            @size-change="handleUsersPageSizeChange"
          />
        </div>
      </section>

      <section class="attribution-panel">
        <header class="attribution-panel-header">
          <span>{{ t('modelSummary') }}</span>
          <small>{{ t('defaultSortByCost') }}</small>
        </header>
        <el-table
          v-loading="modelsLoading"
          class="admin-table service-table attribution-table"
          :data="models.items"
          :row-key="modelRowKey"
          stripe
        >
          <el-table-column :label="t('channelAndModel')" min-width="220">
            <template #default="{ row }">
              <div class="attribution-model-cell">
                <span class="usage-provider">{{ row.channel_name || '-' }}</span>
                <span class="usage-separator">/</span>
                <span>{{ row.model || '-' }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('billingMeter')" min-width="110">
            <template #default="{ row }">{{ billingMeterLabel(row.billing_meter) }}</template>
          </el-table-column>
          <el-table-column :label="t('requestCount')" min-width="120" align="right">
            <template #default="{ row }">{{ formatNumber(row.request_count, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('tokens')" min-width="130" align="right">
            <template #default="{ row }">{{ formatNumber(row.total_tokens, locale) }}</template>
          </el-table-column>
          <el-table-column :label="t('cost')" min-width="120" align="right">
            <template #default="{ row }">{{ formatMoney(row.cost_micros, locale, 6) }}</template>
          </el-table-column>
          <el-table-column :label="t('averageLatencyShort')" min-width="120" align="right">
            <template #default="{ row }">{{ formatDurationMs(row.avg_latency_ms) }}</template>
          </el-table-column>
          <el-table-column :label="t('userCount')" min-width="100" align="right">
            <template #default="{ row }">{{ formatNumber(row.user_count, locale) }}</template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noStatisticsData')" />
          </template>
        </el-table>
        <div class="attribution-pagination">
          <el-pagination
            v-model:current-page="modelsPage"
            v-model:page-size="modelsPageSize"
            background
            layout="total, sizes, prev, pager, next"
            :total="models.total"
            :page-sizes="[20, 50, 100]"
            @current-change="handleModelsPageChange"
            @size-change="handleModelsPageSizeChange"
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

.attribution-quick-ranges {
  align-items: center;
  display: flex;
  gap: 4px;
  grid-column: 1 / -1;
}

.attribution-toolbar-actions {
  align-self: start;
  display: flex;
  justify-content: flex-end;
}

.attribution-toolbar .usage-search-input.el-input {
  flex-basis: 150px;
  width: 150px;
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

.attribution-quick-ranges .el-button.is-active {
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

.attribution-breakdown {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(4, minmax(120px, 1fr));
}

.attribution-breakdown > div {
  background: #f8fafc;
  border: 1px solid #e4ebf3;
  border-radius: 8px;
  display: grid;
  gap: 6px;
  min-width: 0;
  padding: 12px;
}

.attribution-breakdown span {
  color: #667085;
  font-size: 12px;
  font-weight: 640;
}

.attribution-breakdown strong {
  color: #101828;
  font-feature-settings: 'tnum';
  font-size: 16px;
  font-variant-numeric: tabular-nums;
  font-weight: 740;
  overflow-wrap: anywhere;
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
}

.attribution-model-cell {
  align-items: center;
  display: flex;
  gap: 4px;
  min-width: 0;
}

.attribution-model-cell > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-subtable {
  min-width: 0;
}

.attribution-subheader {
  align-items: center;
  display: flex;
  gap: 10px;
  justify-content: space-between;
  margin-bottom: 10px;
  min-width: 0;
}

.attribution-subheader strong {
  color: #1d2939;
  font-size: 14px;
  font-weight: 700;
}

.attribution-subheader span {
  color: #98a2b3;
  font-size: 12px;
  font-weight: 560;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attribution-pagination {
  display: flex;
  justify-content: flex-end;
  padding-top: 16px;
}

@media (max-width: 760px) {
  .attribution-toolbar,
  .attribution-panel {
    padding: 14px;
  }

  .attribution-toolbar {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .attribution-toolbar-actions {
    justify-content: flex-start;
  }

  .attribution-breakdown {
    grid-template-columns: 1fr;
  }

  .attribution-quick-ranges {
    flex-wrap: wrap;
    width: 100%;
  }

  .attribution-quick-ranges .el-button {
    flex: 1 1 0;
    height: 32px;
    min-width: 0;
  }

  .attribution-pagination {
    justify-content: flex-start;
    overflow-x: auto;
  }
}
</style>
