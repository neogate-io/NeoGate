<script setup lang="ts">
import { onMounted, ref } from 'vue'
import {
  CircleCheckFilled,
  Delete,
  Edit,
  Plus,
  Promotion,
  VideoPause,
  View
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { deleteApp, getAppRunLogs, getApps, testApp, updateApp } from '../../api/apps'
import AppCreateDialog from '../../components/admin/apps/AppCreateDialog.vue'
import AppDetailDrawer from '../../components/admin/apps/AppDetailDrawer.vue'
import { useAppCreate } from '../../composables/useAppCreate'
import { useBillingCurrency } from '../../composables/useBillingCurrency'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import type { AppRecord, AppRunLog } from '../../types/admin'
import { copyTextWithMessage } from '../../utils/clipboard'
import { createConfirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'

const { locale, t } = useLocale()
const { formatMoney } = useBillingCurrency()
const confirmDialog = createConfirmAction(() => t('cancel'))

const apps = ref<AppRecord[]>([])
const logs = ref<AppRunLog[]>([])
const loading = ref(false)
const logsLoading = ref(false)
const createOpen = ref(false)
const editOpen = ref(false)
const editSaving = ref(false)
const detailOpen = ref(false)
const activeDetailTab = ref('overview')
const selectedApp = ref<AppRecord | null>(null)
const create = useAppCreate()
const edit = useAppCreate()

function cost(value: number) {
  return formatMoney(value, locale.value, 4)
}

function openCreate() {
  create.resetForm()
  createOpen.value = true
}

function replaceApp(nextApp: AppRecord) {
  const index = apps.value.findIndex((app) => app.id === nextApp.id)
  if (index >= 0) apps.value[index] = nextApp
  if (selectedApp.value?.id === nextApp.id) selectedApp.value = nextApp
}

async function load() {
  await withLoading(loading, async () => {
    try {
      const [nextApps] = await Promise.all([getApps(), create.loadModelOptions()])
      edit.modelOptions.value = create.modelOptions.value
      apps.value = nextApps
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function showCreatedAppDetail(app: AppRecord) {
  createOpen.value = false
  await openDetail(app)
}

async function toggleApp(app: AppRecord) {
  try {
    const status = app.status === 'enabled' ? 'disabled' : 'enabled'
    const nextApp = await updateApp(app.id, {
      status,
      endpoint: { enabled: status === 'enabled' }
    })
    ElMessage.success('应用状态已更新。')
    replaceApp(nextApp)
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function openEdit(app: AppRecord) {
  selectedApp.value = app
  try {
    await edit.loadModelOptions(app.user_key_id)
  } catch (err) {
    edit.modelOptions.value = create.modelOptions.value
    ElMessage.error(readError(err))
  }
  edit.fillFromApp(app)
  editOpen.value = true
}

async function saveApp() {
  const app = selectedApp.value
  if (!app) return
  await withLoading(editSaving, async () => {
    try {
      const nextApp = await updateApp(app.id, edit.updatePayload())
      replaceApp(nextApp)
      editOpen.value = false
      ElMessage.success('应用已保存。')
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function removeApp(app: AppRecord) {
  const confirmed = await confirmDialog(
    `确认删除${create.typeLabel(app.app_type)}“${app.name}”吗？`,
    '删除应用',
    {
      confirmText: '删除',
      danger: true,
      type: 'warning'
    }
  )
  if (!confirmed) return
  try {
    await deleteApp(app.id)
    ElMessage.success('应用已删除。')
    await load()
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function testSelectedApp() {
  if (!selectedApp.value) return
  try {
    await testApp(selectedApp.value.id)
    ElMessage.success('连接配置可用。')
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function openDetail(app: AppRecord) {
  selectedApp.value = app
  activeDetailTab.value = 'overview'
  detailOpen.value = true
  await loadLogs(app.id)
}

async function loadLogs(appId = selectedApp.value?.id) {
  if (!appId) return
  await withLoading(logsLoading, async () => {
    try {
      logs.value = await getAppRunLogs({ appId, limit: 100 })
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function copyText(value?: string | null) {
  if (!value) return
  await copyTextWithMessage(value, '已复制。')
}

onMounted(load)
</script>

<template>
  <section class="grid apps-view">
    <div class="table-toolbar admin-page-toolbar apps-toolbar">
      <div class="admin-page-toolbar-actions">
        <el-button class="admin-action-button" type="primary" :icon="Plus" @click="openCreate">
          新建应用
        </el-button>
      </div>
    </div>

    <div v-loading="loading" class="apps-grid">
      <div v-if="!loading && apps.length === 0" class="apps-empty">
        <el-icon><Promotion /></el-icon>
        <p>暂无应用</p>
      </div>

      <article
        v-for="app in apps"
        :key="app.id"
        class="app-card"
        :class="{ 'is-disabled': app.status !== 'enabled' }"
      >
        <header class="app-card-header">
          <span class="app-type-icon">
            <img :src="create.typeMeta(app.app_type).iconUrl" alt="" />
          </span>
          <div class="app-card-title">
            <h3>{{ app.name }}</h3>
            <span>{{ create.typeLabel(app.app_type) }}</span>
          </div>
          <button
            type="button"
            class="channel-runtime-switch app-status-switch"
            :class="{
              'is-enabled': app.status === 'enabled',
              'is-disabled': app.status !== 'enabled'
            }"
            :aria-pressed="app.status === 'enabled'"
            :aria-label="app.status === 'enabled' ? '启用' : '禁用'"
            @click="toggleApp(app)"
          >
            <span class="channel-runtime-switch-icon">
              <el-icon>
                <CircleCheckFilled v-if="app.status === 'enabled'" />
                <VideoPause v-else />
              </el-icon>
            </span>
            <span class="channel-runtime-switch-text">
              {{ app.status === 'enabled' ? '启用' : '禁用' }}
            </span>
          </button>
        </header>
        <dl class="app-card-metrics">
          <div class="app-card-model">
            <dt>模型</dt>
            <dd>{{ app.model }}</dd>
          </div>
          <div>
            <dt>消息</dt>
            <dd>{{ app.today_message_count }}</dd>
          </div>
          <div>
            <dt>消耗</dt>
            <dd>{{ cost(app.today_cost_micros) }}</dd>
          </div>
        </dl>
        <div class="app-actions table-row-actions">
          <el-tooltip content="详情" placement="top" :show-after="600">
            <el-button
              class="admin-action-button icon-only-action"
              :icon="View"
              @click="openDetail(app)"
            />
          </el-tooltip>
          <el-tooltip content="编辑" placement="top" :show-after="600">
            <el-button
              class="admin-action-button icon-only-action"
              :icon="Edit"
              @click="openEdit(app)"
            />
          </el-tooltip>
          <el-tooltip content="删除" placement="top" :show-after="600">
            <el-button
              class="admin-action-button icon-only-action"
              type="danger"
              :icon="Delete"
              @click="removeApp(app)"
            />
          </el-tooltip>
        </div>
      </article>
    </div>

    <AppCreateDialog
      v-model:open="createOpen"
      :create="create"
      @copy="copyText"
      @show-detail="showCreatedAppDetail"
    />

    <AppCreateDialog
      v-model:open="editOpen"
      :create="edit"
      mode="edit"
      :saving="editSaving"
      @save="saveApp"
      @copy="copyText"
      @show-detail="showCreatedAppDetail"
    />

    <AppDetailDrawer
      v-model:open="detailOpen"
      v-model:active-tab="activeDetailTab"
      :app="selectedApp"
      :logs="logs"
      :logs-loading="logsLoading"
      @copy="copyText"
      @test="testSelectedApp"
    />
  </section>
</template>

<style scoped>
.apps-view {
  display: grid;
  gap: 16px;
}

.apps-toolbar {
  align-items: center;
  display: flex;
  gap: 14px;
  justify-content: space-between;
}

.apps-grid {
  display: grid;
  gap: 14px;
  grid-template-columns: repeat(auto-fill, 320px);
  justify-content: start;
  min-height: 220px;
}

.apps-empty {
  align-items: center;
  background: #fff;
  border: 1px dashed var(--admin-border);
  border-radius: 8px;
  color: var(--admin-text-muted);
  display: grid;
  gap: 10px;
  justify-items: center;
  min-height: 220px;
  padding: 28px;
}

.apps-empty .el-icon {
  font-size: 28px;
}

.app-card {
  background: #ffffff;
  border: 1px solid var(--admin-border);
  border-radius: 8px;
  box-shadow: var(--admin-shadow);
  display: grid;
  gap: 12px;
  grid-template-rows: auto 1fr auto;
  min-height: 0;
  overflow: hidden;
  padding: 12px;
  transition:
    background-color 160ms ease,
    border-color 160ms ease,
    box-shadow 160ms ease;
  width: 320px;
}

.app-card:hover {
  background: #fbfdff;
  border-color: #c8d4e2;
  box-shadow: 0 8px 20px rgba(15, 23, 42, 0.055);
}

.app-card.is-disabled {
  background: #f8fafc;
  border-color: #e2e8f0;
  box-shadow: none;
}

.app-card.is-disabled .app-type-icon,
.app-card.is-disabled .app-card-title,
.app-card.is-disabled .app-card-metrics,
.app-card.is-disabled .app-actions {
  opacity: 0.58;
}

.app-card.is-disabled:hover {
  background: #f8fafc;
  border-color: #e2e8f0;
  box-shadow: none;
}

.app-card-header {
  align-items: center;
  display: grid;
  gap: 8px;
  grid-template-columns: auto minmax(0, 1fr) auto;
}

.app-type-icon {
  align-items: center;
  color: var(--admin-primary);
  display: inline-flex;
  height: 46px;
  justify-content: center;
  width: 46px;
}

.app-type-icon img {
  display: block;
  height: 36px;
  width: 36px;
}

.app-status-switch {
  min-width: 78px;
}

.app-status-switch.is-enabled,
.app-status-switch.is-enabled .channel-runtime-switch-text {
  background: var(--admin-success-bg);
  border-color: var(--admin-success-border);
  color: var(--admin-success);
}

.app-status-switch.is-disabled,
.app-status-switch.is-disabled .channel-runtime-switch-text {
  background: #f1f5f9;
  border-color: #e2e8f0;
  color: #64748b;
}

.app-status-switch.is-enabled .channel-runtime-switch-icon {
  background: #22c55e;
}

.app-status-switch.is-disabled .channel-runtime-switch-icon {
  background: #94a3b8;
}

.app-status-switch,
.app-status-switch * {
  transition: none;
}

.app-card-title {
  min-width: 0;
}

.app-card-title h3 {
  color: var(--admin-heading);
  font-size: 15px;
  font-weight: 600;
  line-height: 1.25;
  margin: 0 0 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-card-title span,
.app-card-metrics dt {
  color: var(--admin-text-muted);
}

.app-card-title span {
  display: block;
  font-size: 13px;
  line-height: 1.25;
}

.app-card-metrics {
  display: grid;
  background: #f8fafc;
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  gap: 0;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin: 0;
}

.app-card-metrics > div {
  min-width: 0;
  padding: 8px 10px;
}

.app-card-metrics > div + div {
  border-left: 1px solid var(--admin-border-soft);
}

.app-card-metrics > div:nth-child(2) {
  border-left: 0;
}

.app-card-model {
  border-bottom: 1px solid var(--admin-border-soft);
  grid-column: 1 / -1;
}

.app-card-metrics dt {
  font-size: 12px;
}

.app-card-metrics dd {
  color: var(--admin-text);
  font-size: 13px;
  font-weight: 650;
  margin: 3px 0 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-actions {
  display: flex;
  justify-content: flex-end;
}

@media (max-width: 760px) {
  .apps-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .apps-grid {
    grid-template-columns: 1fr;
  }

  .app-card {
    width: 100%;
  }
}
</style>
