<script setup lang="ts">
import {
  CircleCheckFilled,
  CircleCloseFilled,
  Loading,
  VideoPause,
  WarningFilled
} from '@element-plus/icons-vue'
import type { UseChannelDiagnostics } from '../../../composables/useChannelDiagnostics'
import { useLocale } from '../../../composables/useLocale'
import type {
  Channel,
  ChannelDiagnosticReport,
  DiagnosticStep,
  DiagnosticStatus,
  EndpointDiagnosticReport
} from '../../../types/admin'
import { formatDurationMs } from '../../../utils/format'

const props = defineProps<{
  diagnostic: UseChannelDiagnostics
}>()
const diagnostic = props.diagnostic

const emit = defineEmits<{
  retry: [channel: Channel]
}>()

const { t } = useLocale()

function diagnosticStatusLabel(status: DiagnosticStatus) {
  const labels: Record<DiagnosticStatus, string> = {
    ok: t('diagnosticStatusOk'),
    warning: t('diagnosticStatusWarning'),
    failed: t('diagnosticStatusFailed'),
    skipped: t('diagnosticStatusSkipped')
  }
  return labels[status]
}

function diagnosticStatusType(status: DiagnosticStatus) {
  const types: Record<DiagnosticStatus, 'success' | 'warning' | 'danger' | 'info'> = {
    ok: 'success',
    warning: 'warning',
    failed: 'danger',
    skipped: 'info'
  }
  return types[status]
}

function diagnosticStepIcon(status: DiagnosticStatus) {
  const icons = {
    ok: CircleCheckFilled,
    warning: WarningFilled,
    failed: CircleCloseFilled,
    skipped: VideoPause
  }
  return icons[status]
}

function diagnosticStepLabel(name: string) {
  if (name === 'models') return t('diagnosticStepModels')
  if (name === 'probe') return t('diagnosticStepProbe')
  if (name.startsWith('probe:')) return `${t('diagnosticStepProbe')} · ${name.slice(6)}`
  return name
}

function diagnosticModelsPreview(models: string[]) {
  if (models.length === 0) return t('diagnosticNoModels')
  return models.slice(0, 6).join(', ') + (models.length > 6 ? ` +${models.length - 6}` : '')
}

function diagnosticEndpointCount(report: ChannelDiagnosticReport) {
  return report.endpoints.length
}

function diagnosticKeyCount(report: ChannelDiagnosticReport) {
  return report.endpoints.reduce((count, endpoint) => count + endpoint.keys.length, 0)
}

function diagnosticAvailableKeyCount(report: ChannelDiagnosticReport) {
  return report.endpoints.reduce(
    (count, endpoint) => count + endpoint.keys.filter((key) => key.status === 'ok').length,
    0
  )
}

function diagnosticConfiguredModelCount(endpoint: EndpointDiagnosticReport) {
  return endpoint.configured_models.length
}

function diagnosticDiscoveredModelSummary(endpoint: EndpointDiagnosticReport) {
  const count = endpoint.discovered_models.length
  return count > 0 ? `${count}` : t('diagnosticNoModels')
}

function diagnosticStepMeta(step: DiagnosticStep) {
  return `${formatDurationMs(step.duration_ms)}${step.status_code ? ` · HTTP ${step.status_code}` : ''}`
}

function diagnosticEndpointTitle(endpoint: EndpointDiagnosticReport) {
  return `${endpoint.protocol.toUpperCase()} · ${endpoint.base_url}`
}

function setLiveListRef(element: unknown) {
  diagnostic.liveListRef.value = element instanceof HTMLElement ? element : null
}
</script>

<template>
  <el-dialog
    v-model="diagnostic.dialogOpen.value"
    class="channel-dialog diagnostic-dialog"
    :title="t('channelDiagnosticReport')"
    width="760px"
    :close-on-click-modal="!diagnostic.inProgress.value"
    :close-on-press-escape="!diagnostic.inProgress.value"
  >
    <div v-if="diagnostic.inProgress.value && diagnostic.channel.value" class="diagnostic-running">
      <div class="diagnostic-running-icon">
        <el-icon><Loading /></el-icon>
      </div>
      <div class="diagnostic-running-copy">
        <strong>{{ t('diagnosticRunningTitle') }}</strong>
        <span>{{ diagnostic.channel.value.name }} · {{ diagnostic.channel.value.provider }}</span>
        <p>{{ t('diagnosticRunningHint') }}</p>
      </div>
      <div v-if="diagnostic.currentModel.value" class="diagnostic-current-model">
        <span>{{ t('diagnosticCurrentModel') }}</span>
        <strong>{{ diagnostic.currentModel.value }}</strong>
      </div>
      <div class="diagnostic-live-panel">
        <div :ref="setLiveListRef" class="diagnostic-live-list">
          <div v-if="diagnostic.liveSteps.value.length === 0" class="diagnostic-live-empty">
            {{ t('diagnosticWaitingFirstResult') }}
          </div>
          <div
            v-for="event in diagnostic.liveSteps.value"
            :key="`${event.endpoint_id}-${event.key_id ?? event.key_name}-${event.model}`"
            class="diagnostic-step"
            :class="`is-${event.step.status}`"
          >
            <el-icon class="diagnostic-step-icon">
              <component :is="diagnosticStepIcon(event.step.status)" />
            </el-icon>
            <div class="diagnostic-step-copy">
              <strong>{{ diagnosticStepLabel(event.step.name) }}</strong>
              <span>{{ event.step.message }}</span>
            </div>
            <span class="diagnostic-step-meta">{{ diagnosticStepMeta(event.step) }}</span>
          </div>
        </div>
      </div>
    </div>

    <div v-else-if="diagnostic.error.value" class="diagnostic-error">
      <el-alert
        :title="t('diagnosticFailedTitle')"
        :description="diagnostic.error.value"
        type="error"
        show-icon
      />
      <el-button
        v-if="diagnostic.channel.value"
        class="admin-action-button"
        type="primary"
        @click="emit('retry', diagnostic.channel.value)"
      >
        {{ t('retry') }}
      </el-button>
    </div>

    <div v-else-if="diagnostic.report.value" class="diagnostic-report">
      <div class="diagnostic-result-card" :class="`is-${diagnostic.report.value.status}`">
        <div class="diagnostic-result-main">
          <span>{{ t('diagnosticResultOverview') }}</span>
          <strong>{{ diagnostic.report.value.summary }}</strong>
          <small>
            {{ diagnostic.report.value.channel_name }} · {{ diagnostic.report.value.provider }}
          </small>
        </div>
        <el-tag :type="diagnosticStatusType(diagnostic.report.value.status)" effect="light" round>
          {{ diagnosticStatusLabel(diagnostic.report.value.status) }}
        </el-tag>
      </div>

      <div class="diagnostic-stats">
        <div class="diagnostic-stat">
          <span>{{ t('latency') }}</span>
          <strong>{{ formatDurationMs(diagnostic.report.value.duration_ms) }}</strong>
        </div>
        <div class="diagnostic-stat">
          <span>{{ t('diagnosticTestedEndpoints') }}</span>
          <strong>{{ diagnosticEndpointCount(diagnostic.report.value) }}</strong>
        </div>
        <div class="diagnostic-stat">
          <span>{{ t('diagnosticTestedKeys') }}</span>
          <strong>{{ diagnosticKeyCount(diagnostic.report.value) }}</strong>
        </div>
        <div class="diagnostic-stat">
          <span>{{ t('diagnosticAvailableKeys') }}</span>
          <strong>{{ diagnosticAvailableKeyCount(diagnostic.report.value) }}</strong>
        </div>
      </div>

      <div class="diagnostic-section">
        <div class="diagnostic-section-title">
          <strong>{{ t('diagnosticEndpointOverview') }}</strong>
        </div>
        <div
          v-for="endpoint in diagnostic.report.value.endpoints"
          :key="endpoint.endpoint_id"
          class="diagnostic-endpoint-card"
        >
          <div class="diagnostic-endpoint-head">
            <div>
              <strong>{{ diagnosticEndpointTitle(endpoint) }}</strong>
              <span>{{ endpoint.summary }}</span>
            </div>
            <el-tag :type="diagnosticStatusType(endpoint.status)" size="small" effect="light">
              {{ diagnosticStatusLabel(endpoint.status) }}
            </el-tag>
          </div>
          <div class="diagnostic-endpoint-facts">
            <span>
              {{ t('diagnosticConfiguredModels') }}
              <strong>{{ diagnosticConfiguredModelCount(endpoint) }}</strong>
            </span>
            <span>
              {{ t('diagnosticDiscoveredModels') }}
              <strong>{{ diagnosticDiscoveredModelSummary(endpoint) }}</strong>
            </span>
            <span v-if="endpoint.missing_configured_models.length" class="is-warning">
              {{ t('diagnosticMissingModels') }}
              <strong>{{ diagnosticModelsPreview(endpoint.missing_configured_models) }}</strong>
            </span>
          </div>
          <div v-if="endpoint.discovered_models.length" class="diagnostic-model-preview">
            {{ diagnosticModelsPreview(endpoint.discovered_models) }}
          </div>
        </div>
      </div>

      <div class="diagnostic-section">
        <div class="diagnostic-section-title">
          <strong>{{ t('diagnosticKeyChecks') }}</strong>
        </div>
        <div
          v-for="endpoint in diagnostic.report.value.endpoints"
          :key="`keys-${endpoint.endpoint_id}`"
          class="diagnostic-key-group"
        >
          <div
            v-for="key in endpoint.keys"
            :key="`${endpoint.endpoint_id}-${key.key_id ?? key.key_name}`"
            class="diagnostic-key-item"
          >
            <div class="diagnostic-key-head">
              <div>
                <strong>{{ key.key_name }}</strong>
                <span v-if="key.key_prefix">{{ key.key_prefix }}</span>
                <span v-else>{{ endpoint.protocol.toUpperCase() }}</span>
              </div>
              <el-tag :type="diagnosticStatusType(key.status)" size="small" effect="light">
                {{ diagnosticStatusLabel(key.status) }}
              </el-tag>
            </div>
            <p>{{ key.summary }}</p>
            <div class="diagnostic-step-list">
              <div
                v-for="step in key.steps"
                :key="`${key.key_id ?? key.key_name}-${step.name}`"
                class="diagnostic-step"
                :class="`is-${step.status}`"
              >
                <el-icon class="diagnostic-step-icon">
                  <component :is="diagnosticStepIcon(step.status)" />
                </el-icon>
                <div class="diagnostic-step-copy">
                  <strong>{{ diagnosticStepLabel(step.name) }}</strong>
                  <span>{{ step.message }}</span>
                </div>
                <span class="diagnostic-step-meta">{{ diagnosticStepMeta(step) }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<style scoped>
.diagnostic-report {
  display: grid;
  gap: 16px;
}

.diagnostic-running,
.diagnostic-error {
  display: grid;
  gap: 14px;
  min-height: 240px;
  place-items: center;
  text-align: center;
}

.diagnostic-running-copy {
  display: grid;
  gap: 6px;
  justify-items: center;
  max-width: 520px;
}

.diagnostic-running-copy strong {
  color: #1d2129;
  font-size: 16px;
  font-weight: 680;
}

.diagnostic-running-copy span {
  color: #86909c;
  font-size: 13px;
}

.diagnostic-running-copy p {
  color: #4e5969;
  line-height: 1.55;
  margin: 0;
}

.diagnostic-current-model {
  align-items: center;
  background: #fff7ed;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  display: flex;
  gap: 8px;
  max-width: min(100%, 620px);
  padding: 10px 12px;
}

.diagnostic-current-model span {
  color: #c2410c;
  font-size: 12px;
  font-weight: 720;
  white-space: nowrap;
}

.diagnostic-current-model strong {
  color: #1d2129;
  font-size: 13px;
  font-weight: 760;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.diagnostic-live-panel {
  background: #ffffff;
  border: 1px solid #d8e0ea;
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(16, 24, 40, 0.04);
  box-sizing: border-box;
  height: 302px;
  padding: 0 0 0 10px;
  width: min(100%, 620px);
}

.diagnostic-live-list {
  align-content: start;
  box-sizing: border-box;
  display: grid;
  gap: 8px;
  grid-auto-rows: max-content;
  height: 100%;
  overflow: auto;
  padding: 10px 8px 10px 0;
  scrollbar-gutter: stable;
  width: 100%;
}

.diagnostic-live-empty {
  color: #667085;
  font-size: 13px;
  font-weight: 620;
  line-height: 1.5;
  padding: 2px 0;
  text-align: center;
}

.diagnostic-running-icon {
  align-items: center;
  background: #fff7ed;
  border-radius: 999px;
  color: #c2410c;
  display: inline-flex;
  font-size: 22px;
  height: 56px;
  justify-content: center;
  width: 56px;
}

.diagnostic-running-icon :deep(.el-icon) {
  animation: diagnostic-spin 1s linear infinite;
}

@keyframes diagnostic-spin {
  to {
    transform: rotate(360deg);
  }
}

.diagnostic-result-card {
  align-items: start;
  background: #f7fbf8;
  border: 1px solid #d8f0d1;
  border-radius: 8px;
  display: flex;
  gap: 14px;
  justify-content: space-between;
  padding: 14px 16px;
}

.diagnostic-result-card.is-warning {
  background: #fffaf0;
  border-color: #fdecc8;
}

.diagnostic-result-card.is-failed {
  background: #fff7f7;
  border-color: #ffd6d6;
}

.diagnostic-result-main {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.diagnostic-result-main span,
.diagnostic-section-title span,
.diagnostic-stat span,
.diagnostic-key-head span,
.diagnostic-step-copy span,
.diagnostic-step-meta,
.diagnostic-endpoint-head span,
.diagnostic-model-preview,
.diagnostic-endpoint-facts {
  color: #86909c;
  font-size: 12px;
}

.diagnostic-result-main strong {
  color: #1d2129;
  font-size: 16px;
  font-weight: 760;
  line-height: 1.35;
}

.diagnostic-result-main small {
  color: #667085;
  font-size: 12px;
  font-weight: 620;
}

.diagnostic-stats {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(4, minmax(0, 1fr));
}

.diagnostic-stat {
  background: #f8fafc;
  border: 1px solid #edf1f6;
  border-radius: 8px;
  display: grid;
  gap: 4px;
  padding: 10px 12px;
}

.diagnostic-stat strong {
  color: #1d2129;
  font-size: 16px;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 760;
}

.diagnostic-section {
  display: grid;
  gap: 10px;
}

.diagnostic-section-title {
  align-items: center;
  display: flex;
  justify-content: space-between;
}

.diagnostic-section-title strong {
  color: #344054;
  font-size: 14px;
  font-weight: 760;
}

.diagnostic-endpoint-card,
.diagnostic-key-item {
  border: 1px solid #e6edf5;
  border-radius: 8px;
  display: grid;
  gap: 12px;
  padding: 13px 14px;
}

.diagnostic-endpoint-head,
.diagnostic-key-head {
  align-items: start;
  display: flex;
  gap: 12px;
  justify-content: space-between;
}

.diagnostic-endpoint-head div,
.diagnostic-key-head div {
  display: grid;
  gap: 4px;
  min-width: 0;
}

.diagnostic-endpoint-head strong,
.diagnostic-key-head strong {
  color: #1d2129;
  font-size: 14px;
  font-weight: 760;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.diagnostic-endpoint-facts {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px 14px;
}

.diagnostic-endpoint-facts strong {
  color: #344054;
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  font-weight: 760;
  margin-left: 4px;
}

.diagnostic-endpoint-facts .is-warning,
.diagnostic-endpoint-facts .is-warning strong {
  color: #c2410c;
}

.diagnostic-model-preview {
  background: #f8fafc;
  border-radius: 8px;
  line-height: 1.45;
  overflow-wrap: anywhere;
  padding: 9px 10px;
}

.diagnostic-key-group {
  display: grid;
  gap: 10px;
}

.diagnostic-key-item p {
  color: #4e5969;
  line-height: 1.45;
  margin: 0;
}

.diagnostic-step-list {
  display: grid;
  gap: 8px;
}

.diagnostic-step {
  align-items: center;
  background: #fbfcff;
  border-radius: 8px;
  display: grid;
  gap: 10px;
  grid-template-columns: 18px minmax(0, 1fr) auto;
  min-height: 74px;
  padding: 9px 10px;
}

.diagnostic-step-icon {
  color: #16a34a;
  font-size: 18px;
}

.diagnostic-step.is-warning .diagnostic-step-icon {
  color: #d97706;
}

.diagnostic-step.is-failed .diagnostic-step-icon {
  color: #dc2626;
}

.diagnostic-step.is-skipped .diagnostic-step-icon {
  color: #64748b;
}

.diagnostic-step-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
}

.diagnostic-step-copy strong {
  color: #1d2129;
  font-size: 13px;
  font-weight: 720;
}

.diagnostic-step-copy span {
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.diagnostic-step-meta {
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

@media (max-width: 760px) {
  .diagnostic-stats {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .diagnostic-step {
    align-items: start;
    grid-template-columns: 10px minmax(0, 1fr);
  }

  .diagnostic-step-meta {
    grid-column: 2;
  }

  .diagnostic-result-card,
  .diagnostic-endpoint-head,
  .diagnostic-key-head {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
