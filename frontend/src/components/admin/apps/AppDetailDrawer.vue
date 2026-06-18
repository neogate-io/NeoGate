<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue'
import type { AppRecord, AppRunLog } from '../../../types/admin'
import { formatCompactDateTime } from '../../../utils/format'

const open = defineModel<boolean>('open', { required: true })
const activeTab = defineModel<string>('activeTab', { required: true })

defineProps<{
  app: AppRecord | null
  logs: AppRunLog[]
  logsLoading: boolean
  typeLabel: (type: string) => string
  statusLabel: (status: string) => string
}>()

const emit = defineEmits<{
  copy: [value?: string | null]
  test: []
}>()
</script>

<template>
  <el-drawer v-model="open" size="760px" :title="app?.name || '应用详情'">
    <el-tabs v-if="app" v-model="activeTab">
      <el-tab-pane label="概览" name="overview">
        <dl class="app-detail-list">
          <div><dt>应用类型</dt><dd>{{ typeLabel(app.app_type) }}</dd></div>
          <div><dt>状态</dt><dd>{{ statusLabel(app.status) }}</dd></div>
          <div><dt>默认模型</dt><dd>{{ app.model }}</dd></div>
          <div><dt>绑定 API Key</dt><dd>{{ app.user_key_name }}</dd></div>
          <div><dt>项目</dt><dd>{{ app.project_name }}</dd></div>
          <div><dt>今日消息</dt><dd>{{ app.today_message_count }}</dd></div>
        </dl>
      </el-tab-pane>

      <el-tab-pane label="接入配置" name="endpoint">
        <div v-if="app.endpoint" class="endpoint-panel">
          <el-button class="admin-action-button" :icon="Refresh" @click="emit('test')">
            测试连接
          </el-button>
          <el-descriptions border :column="1">
            <el-descriptions-item label="入口类型">
              {{ app.endpoint.endpoint_type }}
            </el-descriptions-item>
            <el-descriptions-item label="回调 URL">
              <el-button link type="primary" @click="emit('copy', app.endpoint.callback_url)">
                {{ app.endpoint.callback_url || '-' }}
              </el-button>
            </el-descriptions-item>
            <el-descriptions-item label="调用 URL">
              <el-button link type="primary" @click="emit('copy', app.endpoint.invoke_url)">
                {{ app.endpoint.invoke_url || '-' }}
              </el-button>
            </el-descriptions-item>
            <el-descriptions-item v-if="app.endpoint.widget_script_url" label="嵌入脚本">
              <el-button
                link
                type="primary"
                @click="emit('copy', app.endpoint.widget_script_url)"
              >
                {{ app.endpoint.widget_script_url }}
              </el-button>
            </el-descriptions-item>
            <el-descriptions-item label="密钥">
              {{ app.endpoint.secrets_set.length ? app.endpoint.secrets_set.join(', ') : '-' }}
            </el-descriptions-item>
          </el-descriptions>
        </div>
      </el-tab-pane>

      <el-tab-pane label="会话日志" name="logs">
        <el-table v-loading="logsLoading" :data="logs" size="small">
          <el-table-column label="时间" min-width="140">
            <template #default="{ row }: { row: AppRunLog }">
              {{ formatCompactDateTime(row.created_at) }}
            </template>
          </el-table-column>
          <el-table-column prop="status" label="状态" width="96" />
          <el-table-column prop="model" label="模型" min-width="130" />
          <el-table-column prop="external_user_id" label="外部用户" min-width="120" />
          <el-table-column label="Token" width="100">
            <template #default="{ row }: { row: AppRunLog }">{{ row.total_tokens ?? '-' }}</template>
          </el-table-column>
          <el-table-column prop="error_summary" label="错误" min-width="180" show-overflow-tooltip />
        </el-table>
      </el-tab-pane>

      <el-tab-pane label="知识库" name="knowledge" disabled />
      <el-tab-pane label="工具" name="tools" disabled />
      <el-tab-pane label="权限" name="permissions" disabled />
    </el-tabs>
  </el-drawer>
</template>

<style scoped>
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
  .app-detail-list {
    grid-template-columns: 1fr;
  }
}
</style>
