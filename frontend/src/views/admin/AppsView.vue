<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Delete, Plus, Promotion, SwitchButton, View } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { deleteApp, getAppRunLogs, getApps, testApp, updateApp } from '../../api/apps'
import AppCreateDialog from '../../components/admin/apps/AppCreateDialog.vue'
import AppDetailDrawer from '../../components/admin/apps/AppDetailDrawer.vue'
import AdminActionTooltip from '../../components/admin/AdminActionTooltip.vue'
import { statusLabel, useAppCreate } from '../../composables/useAppCreate'
import { useLocale } from '../../composables/useLocale'
import type { AppRecord, AppRunLog } from '../../types/admin'
import { confirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import { formatCompactDateTime, microUsdToUsd } from '../../utils/format'

const { t } = useLocale()

const apps = ref<AppRecord[]>([])
const logs = ref<AppRunLog[]>([])
const loading = ref(false)
const logsLoading = ref(false)
const createOpen = ref(false)
const detailOpen = ref(false)
const activeDetailTab = ref('overview')
const selectedApp = ref<AppRecord | null>(null)
const create = useAppCreate()

const filteredApps = computed(() => apps.value)

function typeMeta(type: string) {
  return create.typeMeta(type)
}

function typeLabel(type: string) {
  return create.typeLabel(type)
}

function cost(value: number) {
  return `$${microUsdToUsd(value).toFixed(4)}`
}

function openCreate() {
  create.resetForm()
  createOpen.value = true
}

async function load() {
  loading.value = true
  try {
    const [nextApps] = await Promise.all([getApps(), create.loadModelOptions()])
    apps.value = nextApps
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function showCreatedAppDetail(app: AppRecord) {
  createOpen.value = false
  await openDetail(app)
}

async function toggleApp(app: AppRecord) {
  try {
    const status = app.status === 'enabled' ? 'disabled' : 'enabled'
    await updateApp(app.id, {
      status,
      endpoint: { enabled: status === 'enabled' }
    })
    ElMessage.success('应用状态已更新。')
    await load()
  } catch (err) {
    ElMessage.error(readError(err))
  }
}

async function removeApp(app: AppRecord) {
  const confirmed = await confirmAction(`确认删除应用「${app.name}」？`, '删除应用', {
    confirmText: '删除',
    cancelText: t('cancel')
  })
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
  logsLoading.value = true
  try {
    logs.value = await getAppRunLogs({ appId, limit: 100 })
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    logsLoading.value = false
  }
}

async function copyText(value?: string | null) {
  if (!value) return
  await navigator.clipboard.writeText(value)
  ElMessage.success('已复制。')
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
      <div v-if="!loading && filteredApps.length === 0" class="apps-empty">
        <el-icon><Promotion /></el-icon>
        <p>暂无应用</p>
      </div>

      <article v-for="app in filteredApps" :key="app.id" class="app-card">
        <header class="app-card-header">
          <span class="app-type-icon">
            <el-icon><component :is="typeMeta(app.app_type).icon" /></el-icon>
          </span>
          <div class="app-card-title">
            <h3>{{ app.name }}</h3>
            <span>{{ typeLabel(app.app_type) }}</span>
          </div>
          <el-tag :type="app.status === 'enabled' ? 'success' : 'info'" round>
            {{ statusLabel(app.status) }}
          </el-tag>
        </header>
        <p class="app-description">{{ app.description || '这个应用还没有描述。' }}</p>
        <dl class="app-card-metrics">
          <div>
            <dt>默认模型</dt>
            <dd>{{ app.model }}</dd>
          </div>
          <div>
            <dt>今日消息</dt>
            <dd>{{ app.today_message_count }}</dd>
          </div>
          <div>
            <dt>今日消耗</dt>
            <dd>{{ cost(app.today_cost_micro_usd) }}</dd>
          </div>
          <div>
            <dt>最近活跃</dt>
            <dd>
              {{
                app.last_active_at
                  ? formatCompactDateTime(app.last_active_at)
                  : '尚未活跃'
              }}
            </dd>
          </div>
        </dl>
        <footer class="app-card-actions">
          <span class="app-updated-at">
            更新 {{ formatCompactDateTime(app.updated_at) }}
          </span>
          <div class="app-actions">
            <AdminActionTooltip content="详情">
              <el-button circle class="app-icon-button" :icon="View" @click="openDetail(app)" />
            </AdminActionTooltip>
            <AdminActionTooltip :content="app.status === 'enabled' ? '禁用' : '启用'">
              <el-button
                circle
                class="app-icon-button"
                :icon="SwitchButton"
                @click="toggleApp(app)"
              />
            </AdminActionTooltip>
            <AdminActionTooltip content="删除">
              <el-button
                circle
                class="app-icon-button is-danger"
                :icon="Delete"
                @click="removeApp(app)"
              />
            </AdminActionTooltip>
          </div>
        </footer>
      </article>
    </div>

    <AppCreateDialog
      v-model:open="createOpen"
      :create="create"
      @copy="copyText"
      @show-detail="showCreatedAppDetail"
    />

    <AppDetailDrawer
      v-model:open="detailOpen"
      v-model:active-tab="activeDetailTab"
      :app="selectedApp"
      :logs="logs"
      :logs-loading="logsLoading"
      :type-label="typeLabel"
      :status-label="statusLabel"
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
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
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
}

.app-card {
  box-shadow: var(--admin-shadow);
  display: grid;
  gap: 12px;
  min-height: 0;
  overflow: hidden;
  padding: 12px;
  transition:
    background-color 160ms ease,
    border-color 160ms ease,
    box-shadow 160ms ease;
  width: 100%;
}

.app-card:hover {
  background: #fbfdff;
  border-color: #c8d4e2;
  box-shadow: 0 8px 20px rgba(15, 23, 42, 0.055);
}

.app-card-header {
  align-items: center;
  display: grid;
  gap: 8px;
  grid-template-columns: auto minmax(0, 1fr) auto;
}

.app-type-icon {
  align-items: center;
  background: var(--admin-primary-soft);
  border-radius: 8px;
  color: var(--admin-primary);
  display: inline-flex;
  height: 34px;
  justify-content: center;
  width: 34px;
}

.app-card-title {
  min-width: 0;
}

.app-card-title h3 {
  color: var(--admin-heading);
  font-size: 14px;
  font-weight: 600;
  line-height: 1.25;
  margin: 0 0 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-card-title span,
.app-description,
.app-card-metrics dt {
  color: var(--admin-text-muted);
}

.app-description {
  display: -webkit-box;
  font-size: 13px;
  line-height: 1.5;
  margin: 0;
  min-height: 40px;
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.app-card-metrics {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin: 0;
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

.app-card-actions {
  align-items: center;
  border-top: 1px solid var(--admin-border-soft);
  display: grid;
  gap: 9px;
  grid-template-columns: minmax(0, 1fr) auto;
  padding-top: 9px;
}

.app-updated-at {
  color: #94a3b8;
  font-size: 12px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-actions {
  display: flex;
  gap: 7px;
  justify-content: flex-end;
  min-width: max-content;
}

.app-icon-button.el-button {
  --el-button-bg-color: #f8fafc;
  --el-button-border-color: var(--admin-border);
  --el-button-hover-bg-color: var(--brand-blue-soft);
  --el-button-hover-border-color: #cbd5e1;
  --el-button-hover-text-color: var(--brand-blue-hover);
  height: 28px;
  width: 28px;
}

.app-icon-button.is-danger.el-button {
  --el-button-hover-bg-color: #fff1f2;
  --el-button-hover-border-color: #fecdd3;
  --el-button-hover-text-color: #e11d48;
}

@media (max-width: 760px) {
  .apps-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .app-card-actions {
    grid-template-columns: 1fr;
  }

  .app-actions {
    justify-content: flex-start;
  }
}
</style>
