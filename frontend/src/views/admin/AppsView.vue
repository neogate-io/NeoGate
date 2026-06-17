<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  ChatDotRound,
  Connection,
  Delete,
  Link,
  Plus,
  Promotion,
  Refresh,
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
import AdminActionTooltip from '../../components/admin/AdminActionTooltip.vue'
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

const appTypes = [
  { type: 'wecom', label: '企业微信应用', icon: ChatDotRound, enabled: true },
  { type: 'feishu', label: '飞书应用', icon: Promotion, enabled: false },
  { type: 'dingtalk', label: '钉钉应用', icon: Promotion, enabled: false },
  { type: 'webhook', label: 'Webhook 应用', icon: Link, enabled: true },
  { type: 'widget', label: '网页组件应用', icon: Connection, enabled: true }
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
    const [nextApps, keyPage] = await Promise.all([getApps(), getUserKeys({ limit: 200 })])
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

    <el-dialog v-model="createOpen" class="app-dialog" title="新建应用" width="680px">
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
        <div class="app-provider-row">
          <el-form-item class="app-type-field" label="应用类型">
            <el-select v-model="form.appType" disabled>
              <template #prefix>
                <el-icon><component :is="typeMeta(form.appType).icon" /></el-icon>
              </template>
              <el-option
                v-for="item in appTypes"
                :key="item.type"
                :label="item.label"
                :value="item.type"
              />
            </el-select>
          </el-form-item>

          <label class="app-status-toggle">
            <span>状态</span>
            <el-switch v-model="form.status" active-value="enabled" inactive-value="disabled" />
          </label>
        </div>

        <el-form-item class="app-name-field" label="应用名称">
          <el-input v-model="form.name" placeholder="例如 研发知识助手" />
        </el-form-item>

        <el-form-item label="描述">
          <el-input v-model="form.description" type="textarea" :rows="2" />
        </el-form-item>

        <div class="app-form-grid">
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
        </div>

        <el-form-item label="系统提示词">
          <el-input v-model="form.systemPrompt" type="textarea" :rows="3" />
        </el-form-item>

        <div class="app-form-divider">接入配置</div>

        <el-form-item label="入口名称">
          <el-input v-model="form.endpointName" placeholder="默认使用应用名称" />
        </el-form-item>

        <template v-if="form.appType === 'wecom'">
          <div class="app-form-grid">
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
          </div>
          <el-form-item label="EncodingAESKey">
            <el-input v-model="form.encodingAesKey" show-password type="password" />
          </el-form-item>
        </template>

        <template v-if="form.appType === 'webhook'">
          <el-form-item label="Webhook Secret">
            <el-input v-model="form.webhookSecret" show-password type="password" />
          </el-form-item>
        </template>

        <template v-if="form.appType === 'widget'">
          <el-form-item label="允许嵌入域名（一行一个）">
            <el-input v-model="form.allowedDomains" type="textarea" :rows="3" />
          </el-form-item>
          <div class="app-form-grid">
            <el-form-item label="欢迎语">
              <el-input v-model="form.welcome" />
            </el-form-item>
            <el-form-item label="主题色">
              <el-color-picker v-model="form.themeColor" />
            </el-form-item>
            <el-form-item label="匿名访问">
              <el-switch v-model="form.anonymousAccess" />
            </el-form-item>
          </div>
        </template>

        <button class="hidden-submit" type="submit" />
      </el-form>

      <template #footer>
        <div class="app-dialog-footer">
          <el-button v-if="form.step === 2" @click="form.step = 1">返回</el-button>
          <el-button @click="createOpen = false">{{ t('cancel') }}</el-button>
          <el-button
            v-if="form.step === 2"
            type="primary"
            :loading="saving"
            @click="submitCreate"
          >
            创建应用
          </el-button>
        </div>
      </template>
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

.app-card,
.app-type-card {
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

.app-type-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
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
  display: grid;
  gap: 13px;
}

.app-provider-row {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 50%) max-content;
  justify-content: start;
}

.app-type-field {
  margin-bottom: 0;
  width: 100%;
}

.app-status-toggle {
  align-items: center;
  border: 1px solid #d8e0ea;
  border-radius: 7px;
  color: #475569;
  display: inline-flex;
  font-size: 13px;
  font-weight: 640;
  gap: 12px;
  min-height: 38px;
  padding: 0 14px;
  white-space: nowrap;
}

.app-form-grid {
  display: grid;
  gap: 13px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.app-form-divider {
  align-items: center;
  color: #64748b;
  display: flex;
  font-size: 12px;
  font-weight: 700;
  gap: 10px;
  letter-spacing: 0;
  margin-top: 2px;
}

.app-form-divider::after {
  background: #edf1f6;
  content: '';
  flex: 1;
  height: 1px;
}

.app-dialog-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.hidden-submit {
  display: none;
}

:global(.app-dialog) {
  border-radius: 8px;
  max-width: calc(100vw - 32px);
}

:global(.app-dialog .el-dialog__header) {
  margin: 0;
  padding: 18px 22px 14px;
}

:global(.app-dialog .el-dialog__title) {
  color: #111827;
  font-size: 18px;
  font-weight: 760;
  line-height: 1.2;
}

:global(.app-dialog .el-dialog__headerbtn) {
  right: 12px;
  top: 10px;
}

:global(.app-dialog .el-dialog__body) {
  padding: 18px 22px;
}

:global(.app-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 22px 18px;
}

.app-create-form :deep(.el-form-item),
.app-type-field {
  margin-bottom: 0;
}

.app-create-form :deep(.el-form-item__label) {
  color: #475569;
  font-size: 13px;
  font-weight: 680;
  line-height: 1.25;
  margin-bottom: 7px;
}

.app-type-field :deep(.el-form-item__content),
.app-name-field :deep(.el-form-item__content) {
  width: 100%;
}

.app-create-form :deep(.el-input__wrapper),
.app-create-form :deep(.el-select__wrapper) {
  border-radius: 7px;
  min-height: 38px;
}

.app-create-form :deep(.el-input__inner) {
  font-size: 14px;
}

.app-create-form :deep(.el-textarea__inner) {
  border-radius: 7px;
  font-size: 14px;
  min-height: 92px;
  padding: 12px 14px;
}

.app-create-form :deep(.el-input-number) {
  width: 100%;
}

.app-dialog-footer :deep(.el-button) {
  border-radius: 7px;
  font-weight: 680;
  min-height: 34px;
  min-width: 86px;
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

  .app-type-grid {
    grid-template-columns: 1fr;
  }

  .app-provider-row {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .app-card-actions {
    grid-template-columns: 1fr;
  }

  .app-actions {
    justify-content: flex-start;
  }

  .app-form-grid,
  .app-detail-list {
    grid-template-columns: 1fr;
  }

  :global(.app-dialog .el-dialog__header) {
    padding: 16px 18px 12px;
  }

  :global(.app-dialog .el-dialog__body) {
    padding: 16px 18px;
  }

  :global(.app-dialog .el-dialog__footer) {
    padding: 12px 18px 16px;
  }

  .app-dialog-footer {
    flex-wrap: wrap;
  }
}
</style>
