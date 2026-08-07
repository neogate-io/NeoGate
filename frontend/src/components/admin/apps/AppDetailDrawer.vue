<script setup lang="ts">
import { Close, Refresh } from '@element-plus/icons-vue'
import type { AppRecord, AppRunLog } from '../../../types/admin'
import { useBillingCurrency } from '../../../composables/useBillingCurrency'
import { useLocale } from '../../../composables/useLocale'
import { formatCompactDateTime } from '../../../utils/format'

const open = defineModel<boolean>('open', { required: true })
const activeTab = defineModel<string>('activeTab', { required: true })

defineProps<{
  app: AppRecord | null
  logs: AppRunLog[]
  logsLoading: boolean
}>()

const emit = defineEmits<{
  copy: [value?: string | null]
  test: []
}>()

const { locale } = useLocale()
const { formatMoney } = useBillingCurrency()

function cost(value: number) {
  return formatMoney(value, locale.value, 4)
}

function logStatusType(status: AppRunLog['status']) {
  if (status === 'success') return 'success'
  if (status === 'failed') return 'danger'
  if (status === 'ignored') return 'info'
  return 'warning'
}
</script>

<template>
  <el-drawer v-model="open" class="app-detail-drawer" size="760px" :with-header="false">
    <div v-if="app" class="app-detail">
      <el-button
        class="app-detail-close"
        :icon="Close"
        circle
        aria-label="关闭"
        @click="open = false"
      />
      <header class="app-detail-hero">
        <div>
          <h2>{{ app.name }}</h2>
          <span>{{ app.description || '这个应用还没有描述。' }}</span>
        </div>
      </header>

      <el-tabs v-model="activeTab" class="app-detail-tabs">
        <el-tab-pane label="概览" name="overview">
          <dl class="app-detail-list">
            <div class="app-detail-model">
              <dt>模型</dt>
              <dd>{{ app.model }}</dd>
            </div>
            <div class="app-detail-stat">
              <dt>今日消息</dt>
              <dd>{{ app.today_message_count }}</dd>
            </div>
            <div class="app-detail-stat">
              <dt>今日消耗</dt>
              <dd>{{ cost(app.today_cost_micros) }}</dd>
            </div>
            <div>
              <dt>上下文轮数</dt>
              <dd>{{ app.context_turns }}</dd>
            </div>
            <div>
              <dt>最大输出 Token</dt>
              <dd>{{ app.max_output_tokens }}</dd>
            </div>
            <div>
              <dt>最近活跃</dt>
              <dd>
                {{ app.last_active_at ? formatCompactDateTime(app.last_active_at) : '尚未活跃' }}
              </dd>
            </div>
          </dl>
        </el-tab-pane>

        <el-tab-pane label="接入配置" name="endpoint">
          <div v-if="app.endpoint" class="endpoint-panel">
            <dl class="endpoint-list">
              <div>
                <dt>入口类型</dt>
                <dd>{{ app.endpoint.endpoint_type }}</dd>
              </div>
              <div v-if="app.endpoint.callback_url">
                <dt>回调 URL</dt>
                <dd>
                  <button type="button" @click="emit('copy', app.endpoint.callback_url)">
                    <span>{{ app.endpoint.callback_url }}</span>
                    <small>点击复制</small>
                  </button>
                </dd>
              </div>
              <div v-if="app.endpoint.invoke_url">
                <dt>调用 URL</dt>
                <dd>
                  <button type="button" @click="emit('copy', app.endpoint.invoke_url)">
                    <span>{{ app.endpoint.invoke_url }}</span>
                    <small>点击复制</small>
                  </button>
                </dd>
              </div>
              <div v-if="app.endpoint.widget_script_url">
                <dt>嵌入脚本</dt>
                <dd>
                  <button type="button" @click="emit('copy', app.endpoint.widget_script_url)">
                    <span>{{ app.endpoint.widget_script_url }}</span>
                    <small>点击复制</small>
                  </button>
                </dd>
              </div>
              <div>
                <dt>密钥</dt>
                <dd>
                  {{ app.endpoint.secrets_set.length ? app.endpoint.secrets_set.join(', ') : '-' }}
                </dd>
              </div>
            </dl>
            <div class="endpoint-actions">
              <el-button class="admin-action-button" :icon="Refresh" @click="emit('test')">
                测试连接
              </el-button>
            </div>
          </div>
        </el-tab-pane>

        <el-tab-pane class="app-log-pane" label="会话日志" name="logs">
          <el-table v-loading="logsLoading" :data="logs" class="app-log-table" size="small">
            <el-table-column label="时间" min-width="128">
              <template #default="{ row }: { row: AppRunLog }">
                {{ formatCompactDateTime(row.created_at) }}
              </template>
            </el-table-column>
            <el-table-column label="状态" width="86">
              <template #default="{ row }: { row: AppRunLog }">
                <el-tag :type="logStatusType(row.status)" round>
                  {{ row.status }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column prop="model" label="模型" min-width="118" show-overflow-tooltip />
            <el-table-column label="外部用户" min-width="96" show-overflow-tooltip>
              <template #default="{ row }: { row: AppRunLog }">
                {{ row.external_user_id || '-' }}
              </template>
            </el-table-column>
            <el-table-column label="Token" width="78" align="right" header-align="right">
              <template #default="{ row }: { row: AppRunLog }">
                {{ row.total_tokens ?? '-' }}
              </template>
            </el-table-column>
            <el-table-column
              prop="error_summary"
              label="错误"
              min-width="130"
              show-overflow-tooltip
            />
            <template #empty>
              <div class="app-log-empty">暂无会话日志</div>
            </template>
          </el-table>
        </el-tab-pane>
      </el-tabs>
    </div>
  </el-drawer>
</template>

<style scoped>
.app-detail {
  display: grid;
  gap: 18px;
  padding: 22px 24px 28px;
  position: relative;
}

.app-detail-close.el-button {
  position: absolute;
  right: 18px;
  top: 18px;
  z-index: 1;
}

.app-detail-hero {
  align-items: flex-start;
  display: grid;
  gap: 16px;
  grid-template-columns: minmax(0, 1fr) auto;
  padding-right: 38px;
}

.app-detail-hero h2 {
  color: var(--admin-heading);
  font-size: 22px;
  line-height: 1.25;
  margin: 0;
}

.app-detail-hero span {
  color: var(--admin-text-muted);
  display: block;
  font-size: 13px;
  line-height: 1.5;
  margin-top: 8px;
}

.app-detail-tabs :deep(.el-tabs__header) {
  margin-bottom: 16px;
}

.app-detail-list {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin: 0;
}

.app-detail-list div {
  background: #f8fafc;
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  min-width: 0;
  padding: 13px 14px;
}

.app-detail-model {
  grid-column: 1 / -1;
}

.app-detail-stat {
  background: #ffffff;
}

.app-detail-list dt {
  color: var(--admin-text-muted);
  font-size: 12px;
}

.app-detail-list dd {
  color: var(--admin-text);
  font-weight: 650;
  margin: 4px 0 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-detail-stat dd {
  color: var(--admin-heading);
  font-size: 20px;
  line-height: 1.15;
  margin-top: 6px;
}

.endpoint-panel {
  display: grid;
  gap: 14px;
}

.endpoint-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  justify-content: flex-end;
}

.endpoint-list {
  background: #ffffff;
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  margin: 0;
  overflow: hidden;
}

.endpoint-list div {
  align-items: center;
  display: grid;
  gap: 12px;
  grid-template-columns: 110px minmax(0, 1fr);
  padding: 12px 14px;
}

.endpoint-list div + div {
  border-top: 1px solid var(--admin-border-soft);
}

.endpoint-list dt {
  color: var(--admin-text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.endpoint-list dd {
  color: var(--admin-text);
  font-size: 13px;
  line-height: 1.45;
  margin: 0;
  min-width: 0;
}

.endpoint-list button {
  align-items: center;
  background: transparent;
  border: 0;
  color: var(--admin-primary);
  cursor: pointer;
  display: grid;
  font: inherit;
  gap: 10px;
  grid-template-columns: minmax(0, 1fr) auto;
  max-width: 100%;
  overflow: hidden;
  padding: 0;
  text-align: left;
  width: 100%;
}

.endpoint-list button:hover {
  color: var(--admin-primary-hover);
}

.endpoint-list button span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.endpoint-list button small {
  color: var(--admin-text-muted);
  font-size: 11px;
  line-height: 1;
}

.app-log-table {
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  max-width: 100%;
  min-width: 0;
  overflow: hidden;
}

.app-log-pane,
.app-log-pane :deep(.el-tab-pane),
.app-log-table :deep(.el-table__inner-wrapper),
.app-log-table :deep(.el-table__body-wrapper),
.app-log-table :deep(.el-scrollbar),
.app-log-table :deep(.el-scrollbar__view) {
  max-width: 100%;
  min-width: 0;
}

.app-log-table :deep(.el-table__inner-wrapper::before) {
  display: none;
}

.app-log-table :deep(.el-table__header th) {
  background: #f8fafc;
  color: var(--admin-text-muted);
  font-size: 12px;
  font-weight: 700;
}

.app-log-table :deep(.el-table__cell) {
  padding: 9px 0;
}

.app-log-empty {
  color: var(--admin-text-muted);
  font-size: 13px;
  padding: 28px 0;
}

@media (max-width: 760px) {
  .app-detail {
    padding: 18px;
  }

  .app-detail-hero {
    grid-template-columns: 1fr;
    padding-right: 36px;
  }

  .app-detail-list {
    grid-template-columns: 1fr;
  }

  .endpoint-list div {
    grid-template-columns: 1fr;
  }
}
</style>
