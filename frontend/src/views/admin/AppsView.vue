<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  ChatDotRound,
  Connection,
  Delete,
  Edit,
  Link,
  Plus,
  Promotion,
  Refresh,
  Search,
  Select,
  SwitchButton,
  View
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  createApp,
  deleteApp,
  getAppRunLogs,
  getApps,
  testApp,
  updateApp,
  type CreateAppInput
} from '../../api/apps'
import { getUserKeys } from '../../api/userKeys'
import { useLocale } from '../../composables/useLocale'
import type { AppRecord, AppRunLog, AppType, UserKey } from '../../types/admin'
import { confirmAction } from '../../utils/confirm'
import { readError } from '../../utils/errors'
import { formatCompactDateTime, microUsdToUsd } from '../../utils/format'

const { t } = useLocale()

const apps = ref<AppRecord[]>([])
const logs = ref<AppRunLog[]>([])
const userKeys = ref<UserKey[]>([])
const loading = ref(false)
const saving = ref(false)
const logsLoading = ref(false)
const createOpen = ref(false)
const detailOpen = ref(false)
const activeDetailTab = ref('overview')
const selectedApp = ref<AppRecord | null>(null)
const search = ref('')
const statusFilter = ref('')
const typeFilter = ref('')

const appTypes = [
  { type: 'wecom', label: '企业微信应用', icon: ChatDotRound, enabled: true },
  { type: 'webhook', label: 'Webhook 应用', icon: Link, enabled: true },
  { type: 'widget', label: '网页组件应用', icon: Connection, enabled: true },
  { type: 'feishu', label: '飞书应用', icon: Promotion, enabled: false },
  { type: 'dingtalk', label: '钉钉应用', icon: Promotion, enabled: false }
] as const

const form = reactive({
  step: 1,
  appType: 'wecom' as AppType,
  name: '',
  description: '',
  status: 'enabled',
  model: '',
  systemPrompt: '',
  contextTurns: 10,
  maxOutputTokens: 2048,
  userKeyId: 0,
  endpointName: '',
  corpId: '',
  agentId: '',
  corpSecret: '',
  callbackToken: '',
  encodingAesKey: '',
  webhookSecret: '',
  allowedDomains: '',
  welcome: '',
  themeColor: '#176baf',
  anonymousAccess: true
})

const filteredApps = computed(() => apps.value)
const selectedEndpoint = computed(() => selectedApp.value?.endpoint ?? null)

function typeMeta(type: string) {
  return appTypes.find((item) => item.type === type) ?? appTypes[0]
}

function statusLabel(status: string) {
  return status === 'enabled' ? '已启用' : '已禁用'
}

function typeLabel(type: string) {
  return typeMeta(type).label
}

function cost(value: number) {
  return `$${microUsdToUsd(value).toFixed(4)}`
}

function resetForm(type: AppType = 'wecom') {
  form.step = 1
  form.appType = type
  form.name = ''
  form.description = ''
  form.status = 'enabled'
  form.model = ''
  form.systemPrompt = ''
  form.contextTurns = 10
  form.maxOutputTokens = 2048
  form.userKeyId = userKeys.value[0]?.id ?? 0
  form.endpointName = ''
  form.corpId = ''
  form.agentId = ''
  form.corpSecret = ''
  form.callbackToken = ''
  form.encodingAesKey = ''
  form.webhookSecret = ''
  form.allowedDomains = ''
  form.welcome = ''
  form.themeColor = '#176baf'
  form.anonymousAccess = true
}

function openCreate() {
  resetForm()
  createOpen.value = true
}

function selectType(type: AppType, enabled: boolean) {
  if (!enabled) {
    ElMessage.info('该应用类型即将支持。')
    return
  }
  form.appType = type
  form.step = 2
}

function endpointConfig() {
  if (form.appType === 'wecom') {
    return {
      corp_id: form.corpId,
      agent_id: form.agentId
    }
  }
  if (form.appType === 'widget') {
    return {
      allowed_domains: form.allowedDomains
        .split('\n')
        .map((item) => item.trim())
        .filter(Boolean),
      welcome: form.welcome,
      theme_color: form.themeColor,
      anonymous_access: form.anonymousAccess
    }
  }
  return {}
}

function endpointSecrets(): Record<string, string> {
  if (form.appType === 'wecom') {
    return {
      corp_secret: form.corpSecret,
      token: form.callbackToken,
      aes_key: form.encodingAesKey
    }
  }
  if (form.appType === 'webhook') {
    return { secret: form.webhookSecret }
  }
  return {}
}

function payload(): CreateAppInput {
  return {
    name: form.name,
    description: form.description,
    app_type: form.appType,
    status: form.status as 'enabled' | 'disabled',
    model: form.model,
    system_prompt: form.systemPrompt,
    context_turns: form.contextTurns,
    max_output_tokens: form.maxOutputTokens,
    user_key_id: form.userKeyId,
    endpoint: {
      name: form.endpointName || form.name,
      enabled: form.status === 'enabled',
      config: endpointConfig(),
      secrets: endpointSecrets()
    }
  }
}

async function load() {
  loading.value = true
  try {
    const [nextApps, keyPage] = await Promise.all([
      getApps({ search: search.value, status: statusFilter.value, appType: typeFilter.value }),
      getUserKeys({ limit: 200 })
    ])
    apps.value = nextApps
    userKeys.value = keyPage.items
    if (!form.userKeyId) form.userKeyId = userKeys.value[0]?.id ?? 0
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function submitCreate() {
  saving.value = true
  try {
    const app = await createApp(payload())
    ElMessage.success('应用已创建。')
    createOpen.value = false
    await load()
    openDetail(app)
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
  }
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
  <section v-loading="loading" class="apps-view">
    <div class="apps-toolbar">
      <div class="apps-toolbar-filters">
        <el-input
          v-model="search"
          clearable
          class="apps-search-input"
          :prefix-icon="Search"
          placeholder="搜索应用名称或描述"
          @keyup.enter="load"
          @clear="load"
        />
        <el-select v-model="statusFilter" clearable placeholder="全部状态" @change="load">
          <el-option label="已启用" value="enabled" />
          <el-option label="已禁用" value="disabled" />
        </el-select>
        <el-select v-model="typeFilter" clearable placeholder="全部类型" @change="load">
          <el-option
            v-for="item in appTypes"
            :key="item.type"
            :label="item.label"
            :value="item.type"
          />
        </el-select>
      </div>
      <div class="apps-toolbar-actions">
        <el-button class="admin-action-button" :icon="Refresh" @click="load">刷新</el-button>
        <el-button class="admin-action-button" type="primary" :icon="Plus" @click="openCreate">
          新建应用
        </el-button>
      </div>
    </div>

    <el-empty v-if="filteredApps.length === 0" description="暂无应用">
      <el-button type="primary" :icon="Plus" @click="openCreate">新建应用</el-button>
    </el-empty>

    <div v-else class="apps-grid">
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
          <el-button class="admin-action-button" :icon="View" @click="openDetail(app)">
            详情
          </el-button>
          <el-button class="admin-action-button" :icon="SwitchButton" @click="toggleApp(app)">
            {{ app.status === 'enabled' ? '禁用' : '启用' }}
          </el-button>
          <el-button class="admin-action-button" :icon="Delete" @click="removeApp(app)">
            删除
          </el-button>
        </footer>
      </article>
    </div>

    <el-dialog v-model="createOpen" title="新建应用" width="860px">
      <div v-if="form.step === 1" class="app-type-grid">
        <button
          v-for="item in appTypes"
          :key="item.type"
          class="app-type-card"
          :class="{ 'is-disabled': !item.enabled }"
          type="button"
          @click="selectType(item.type, item.enabled)"
        >
          <el-icon><component :is="item.icon" /></el-icon>
          <strong>{{ item.label }}</strong>
          <span>{{ item.enabled ? '创建并配置这个应用入口' : '即将支持' }}</span>
        </button>
      </div>

      <el-form v-else class="app-create-form" label-position="top" @submit.prevent="submitCreate">
        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Edit /></el-icon>
            <h3>基础信息</h3>
          </header>
          <div class="admin-settings-grid app-form-grid">
            <el-form-item label="应用名称">
              <el-input v-model="form.name" placeholder="例如 研发知识助手" />
            </el-form-item>
            <el-form-item label="启用状态">
              <el-switch v-model="form.status" active-value="enabled" inactive-value="disabled" />
            </el-form-item>
            <el-form-item class="app-wide-field" label="描述">
              <el-input v-model="form.description" type="textarea" :rows="2" />
            </el-form-item>
          </div>
        </section>

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Connection /></el-icon>
            <h3>模型与计费</h3>
          </header>
          <div class="admin-settings-grid app-form-grid">
            <el-form-item label="默认模型">
              <el-input v-model="form.model" placeholder="例如 gpt-4.1 或 qwen-plus" />
            </el-form-item>
            <el-form-item label="绑定 API Key">
              <el-select v-model="form.userKeyId" filterable placeholder="选择 API Key">
                <el-option
                  v-for="key in userKeys"
                  :key="key.id"
                  :label="`${key.name} · ${key.project_name} · ${key.key_prefix}`"
                  :value="key.id"
                />
              </el-select>
            </el-form-item>
            <el-form-item label="上下文轮数">
              <el-input-number v-model="form.contextTurns" :min="0" :max="50" />
            </el-form-item>
            <el-form-item label="最大输出 Token">
              <el-input-number v-model="form.maxOutputTokens" :min="1" :max="128000" />
            </el-form-item>
            <el-form-item class="app-wide-field" label="系统提示词">
              <el-input v-model="form.systemPrompt" type="textarea" :rows="3" />
            </el-form-item>
          </div>
        </section>

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Link /></el-icon>
            <h3>接入配置</h3>
          </header>
          <div class="admin-settings-grid app-form-grid">
            <el-form-item label="入口名称">
              <el-input v-model="form.endpointName" placeholder="默认使用应用名称" />
            </el-form-item>

            <template v-if="form.appType === 'wecom'">
              <el-form-item label="企业 ID CorpID">
                <el-input v-model="form.corpId" />
              </el-form-item>
              <el-form-item label="应用 AgentID">
                <el-input v-model="form.agentId" />
              </el-form-item>
              <el-form-item label="应用 Secret">
                <el-input v-model="form.corpSecret" show-password type="password" />
              </el-form-item>
              <el-form-item label="回调 Token">
                <el-input v-model="form.callbackToken" show-password type="password" />
              </el-form-item>
              <el-form-item class="app-wide-field" label="EncodingAESKey">
                <el-input v-model="form.encodingAesKey" show-password type="password" />
              </el-form-item>
            </template>

            <template v-if="form.appType === 'webhook'">
              <el-form-item class="app-wide-field" label="Webhook Secret">
                <el-input v-model="form.webhookSecret" show-password type="password" />
              </el-form-item>
            </template>

            <template v-if="form.appType === 'widget'">
              <el-form-item class="app-wide-field" label="允许嵌入域名（一行一个）">
                <el-input v-model="form.allowedDomains" type="textarea" :rows="3" />
              </el-form-item>
              <el-form-item label="欢迎语">
                <el-input v-model="form.welcome" />
              </el-form-item>
              <el-form-item label="主题色">
                <el-color-picker v-model="form.themeColor" />
              </el-form-item>
              <el-form-item label="匿名访问">
                <el-switch v-model="form.anonymousAccess" />
              </el-form-item>
            </template>
          </div>
        </section>

        <div class="admin-settings-actions">
          <el-button class="admin-action-button" @click="form.step = 1">返回</el-button>
          <el-button
            class="admin-action-button"
            native-type="submit"
            type="primary"
            :icon="Select"
            :loading="saving"
          >
            创建应用
          </el-button>
        </div>
      </el-form>
    </el-dialog>

    <el-drawer v-model="detailOpen" size="760px" :title="selectedApp?.name || '应用详情'">
      <el-tabs v-if="selectedApp" v-model="activeDetailTab">
        <el-tab-pane label="概览" name="overview">
          <dl class="app-detail-list">
            <div><dt>应用类型</dt><dd>{{ typeLabel(selectedApp.app_type) }}</dd></div>
            <div><dt>状态</dt><dd>{{ statusLabel(selectedApp.status) }}</dd></div>
            <div><dt>默认模型</dt><dd>{{ selectedApp.model }}</dd></div>
            <div><dt>绑定 API Key</dt><dd>{{ selectedApp.user_key_name }}</dd></div>
            <div><dt>项目</dt><dd>{{ selectedApp.project_name }}</dd></div>
            <div><dt>今日消息</dt><dd>{{ selectedApp.today_message_count }}</dd></div>
          </dl>
        </el-tab-pane>

        <el-tab-pane label="接入配置" name="endpoint">
          <div v-if="selectedEndpoint" class="endpoint-panel">
            <el-button class="admin-action-button" :icon="Refresh" @click="testSelectedApp">
              测试连接
            </el-button>
            <el-descriptions border :column="1">
              <el-descriptions-item label="入口类型">
                {{ selectedEndpoint.endpoint_type }}
              </el-descriptions-item>
              <el-descriptions-item label="回调 URL">
                <el-button link type="primary" @click="copyText(selectedEndpoint.callback_url)">
                  {{ selectedEndpoint.callback_url || '-' }}
                </el-button>
              </el-descriptions-item>
              <el-descriptions-item label="调用 URL">
                <el-button link type="primary" @click="copyText(selectedEndpoint.invoke_url)">
                  {{ selectedEndpoint.invoke_url || '-' }}
                </el-button>
              </el-descriptions-item>
              <el-descriptions-item v-if="selectedEndpoint.widget_script_url" label="嵌入脚本">
                <el-button link type="primary" @click="copyText(selectedEndpoint.widget_script_url)">
                  {{ selectedEndpoint.widget_script_url }}
                </el-button>
              </el-descriptions-item>
              <el-descriptions-item label="密钥">
                {{ selectedEndpoint.secrets_set.length ? selectedEndpoint.secrets_set.join(', ') : '-' }}
              </el-descriptions-item>
            </el-descriptions>
          </div>
        </el-tab-pane>

        <el-tab-pane label="会话日志" name="logs">
          <el-table v-loading="logsLoading" :data="logs" size="small">
            <el-table-column label="时间" min-width="140">
              <template #default="{ row }">
                {{ formatCompactDateTime(row.created_at) }}
              </template>
            </el-table-column>
            <el-table-column prop="status" label="状态" width="96" />
            <el-table-column prop="model" label="模型" min-width="130" />
            <el-table-column prop="external_user_id" label="外部用户" min-width="120" />
            <el-table-column label="Token" width="100">
              <template #default="{ row }">{{ row.total_tokens ?? '-' }}</template>
            </el-table-column>
            <el-table-column prop="error_summary" label="错误" min-width="180" show-overflow-tooltip />
          </el-table>
        </el-tab-pane>

        <el-tab-pane label="知识库" name="knowledge" disabled />
        <el-tab-pane label="工具" name="tools" disabled />
        <el-tab-pane label="权限" name="permissions" disabled />
      </el-tabs>
    </el-drawer>
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

.apps-toolbar-filters,
.apps-toolbar-actions {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.apps-search-input {
  width: 240px;
}

.apps-grid {
  display: grid;
  gap: 14px;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
}

.app-card,
.app-type-card {
  background: var(--admin-surface);
  border: 1px solid var(--admin-border);
  border-radius: 8px;
}

.app-card {
  display: grid;
  gap: 14px;
  min-height: 258px;
  padding: 16px;
}

.app-card-header {
  align-items: center;
  display: grid;
  gap: 10px;
  grid-template-columns: auto minmax(0, 1fr) auto;
}

.app-type-icon {
  align-items: center;
  background: var(--admin-primary-soft);
  border-radius: 8px;
  color: var(--admin-primary);
  display: inline-flex;
  height: 38px;
  justify-content: center;
  width: 38px;
}

.app-card-title {
  min-width: 0;
}

.app-card-title h3 {
  color: var(--admin-heading);
  font-size: 16px;
  margin: 0 0 3px;
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
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.app-type-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
}

.app-type-card {
  cursor: pointer;
  display: grid;
  gap: 8px;
  min-height: 128px;
  padding: 16px;
  text-align: left;
}

.app-type-card .el-icon {
  color: var(--admin-primary);
  font-size: 24px;
}

.app-type-card strong {
  color: var(--admin-heading);
}

.app-type-card span {
  color: var(--admin-text-muted);
  font-size: 13px;
}

.app-type-card.is-disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.app-create-form {
  max-height: 68vh;
  overflow: auto;
}

.app-form-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.app-wide-field {
  grid-column: 1 / -1;
}

.app-detail-list {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.app-detail-list div {
  border-bottom: 1px solid var(--admin-border-soft);
  padding-bottom: 10px;
}

.app-detail-list dt {
  color: var(--admin-text-muted);
  font-size: 12px;
}

.app-detail-list dd {
  color: var(--admin-text);
  font-weight: 650;
  margin: 4px 0 0;
}

.endpoint-panel {
  display: grid;
  gap: 14px;
}

@media (max-width: 760px) {
  .apps-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .apps-search-input,
  .apps-toolbar-filters .el-select {
    width: 100%;
  }

  .app-form-grid,
  .app-detail-list {
    grid-template-columns: 1fr;
  }
}
</style>
