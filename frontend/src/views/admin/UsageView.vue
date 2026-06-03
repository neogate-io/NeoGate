<script setup lang="ts">
import { getUsage } from '../../api/monitoring'
import { useAsyncData } from '../../composables/useAsyncData'
import { useLocale } from '../../composables/useLocale'

const { locale, t } = useLocale()
const { data: usage, loading } = useAsyncData(() => getUsage(), [])

function formatUsd(microUsd?: number | null) {
  if (microUsd == null) return '-'
  return `$${(microUsd / 1_000_000).toFixed(6)}`
}

function formatMs(ms?: number | null) {
  if (ms == null) return '-'
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`
}

function formatNumber(value?: number | null) {
  return value == null ? '-' : value.toLocaleString(locale.value)
}

function cacheWriteTokens(row: { cache_create_in_tokens?: number | null; cache_create_5m_in_tokens?: number | null; cache_create_1h_in_tokens?: number | null }) {
  const split = (row.cache_create_5m_in_tokens || 0) + (row.cache_create_1h_in_tokens || 0)
  return split > 0 ? split : row.cache_create_in_tokens || 0
}

function formatRate(value?: number | null) {
  if (value == null || value <= 0) return null
  return `${Math.round(value).toLocaleString(locale.value)} t/s`
}
</script>

<template>
  <section class="grid usage-view">
    <div class="service-table-panel">
      <el-table v-loading="loading" class="admin-table service-table usage-table" :data="usage" stripe>
        <el-table-column :label="t('time')" min-width="180">
          <template #default="{ row }">{{ new Date(row.created_at).toLocaleString(locale) }}</template>
        </el-table-column>
        <el-table-column :label="t('model')" min-width="190">
          <template #default="{ row }">
            <div class="usage-model">
              <span class="usage-provider">{{ row.provider }}</span>
              <span>{{ row.model || '-' }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('latency')" min-width="170">
          <template #default="{ row }">
            <div class="usage-stack">
              <div class="usage-tags">
                <el-tag type="success" effect="plain" round>{{ formatMs(row.latency_ms) }}</el-tag>
                <el-tag v-if="row.first_response_ms != null" type="success" effect="plain" round>
                  {{ formatMs(row.first_response_ms) }}
                </el-tag>
              </div>
              <span class="usage-muted">
                {{ row.streamed ? t('streamLabel') : t('nonStreamLabel') }}
                <template v-if="formatRate(row.output_tokens_per_second)"> · {{ formatRate(row.output_tokens_per_second) }}</template>
              </span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('tokens')" min-width="180">
          <template #default="{ row }">
            <div class="usage-stack">
              <span class="usage-mono">
                {{ formatNumber(row.input_tokens) }} / {{ formatNumber(row.output_tokens) }}
              </span>
              <span v-if="row.cache_in_tokens || cacheWriteTokens(row) || row.reason_out_tokens" class="usage-muted">
                <template v-if="row.cache_in_tokens">{{ t('cacheRead') }}↓ {{ formatNumber(row.cache_in_tokens) }}</template>
                <template v-if="cacheWriteTokens(row)"> ↑ {{ formatNumber(cacheWriteTokens(row)) }}</template>
                <template v-if="row.reason_out_tokens"> · {{ t('reasoning') }} {{ formatNumber(row.reason_out_tokens) }}</template>
              </span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('cost')">
          <template #default="{ row }">{{ formatUsd(row.cost_micro_usd) }}</template>
        </el-table-column>
        <el-table-column prop="billing_status" :label="t('billing')" />
        <el-table-column :label="t('status')" min-width="100">
          <template #default="{ row }">{{ row.status_code || '-' }}</template>
        </el-table-column>
        <el-table-column prop="error_summary" :label="t('error')" min-width="180">
          <template #default="{ row }">{{ row.error_summary || '-' }}</template>
        </el-table-column>
      </el-table>
    </div>
  </section>
</template>

<style scoped>
.usage-model,
.usage-stack {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.usage-provider,
.usage-muted {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

.usage-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.usage-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-weight: 600;
}
</style>
