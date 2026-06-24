<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Close, CopyDocument, Delete, Plus } from '@element-plus/icons-vue'
import ProviderIcon from '../../common/ProviderIcon.vue'
import { useLocale } from '../../../composables/useLocale'
import type { ChannelForm } from '../../../composables/useChannels'
import type { ChannelKey, EndpointProtocol } from '../../../types/admin'
import type { ChannelProviderOption } from '../../../utils/channel'
import { isManualBaseUrlProvider } from '../../../utils/channel'
import { formatCompactDateTime } from '../../../utils/format'

const open = defineModel<boolean>('open', { required: true })
const form = defineModel<ChannelForm>('form', { required: true })
const baseUrl = defineModel<string>('baseUrl', { required: true })
const secret = defineModel<string>('secret', { required: true })

const props = defineProps<{
  mode: 'create' | 'edit'
  providerOptions: ChannelProviderOption[]
  providerValue: string
  fetchingModels: boolean
  submitting: boolean
  modelsInputPlaceholder: string
  modelsInputReadonly: boolean
  secretPlaceholder: string
  hideCredentialFilesToggle?: boolean
  existingKeys?: ChannelKey[]
  deletingKeyId?: number | null
  copyingKeyId?: number | null
}>()

const emit = defineEmits<{
  fetchModels: []
  selectProvider: [provider: string]
  copyKey: [key: ChannelKey]
  deleteKey: [key: ChannelKey]
  submit: []
}>()

const { t } = useLocale()
const addingKey = ref(false)

const showExistingKeyTable = computed(
  () => props.mode === 'edit' && Boolean(props.existingKeys?.length)
)
const visibleEndpointProtocols = computed<EndpointProtocol[]>(() => {
  if (isManualBaseUrlProvider(form.value.provider)) {
    return ['openai', 'anthropic']
  }

  if (form.value.provider === 'openai' && form.value.use_credentials) {
    return ['openai_oauth']
  }

  const configuredProtocols = (['openai', 'anthropic'] as EndpointProtocol[]).filter((protocol) =>
    form.value.endpoints[protocol].base_url.trim()
  )
  return configuredProtocols.length > 0 ? configuredProtocols : ['openai']
})
const visibleEndpointRows = computed(() =>
  visibleEndpointProtocols.value.map((protocol) => ({
    protocol,
    label: endpointBaseUrlLabel(protocol),
    placeholder: protocol === 'anthropic' ? t('anthropicBaseUrlPlaceholder') : t('baseUrlPlaceholder')
  }))
)
const selectedModels = computed(() =>
  form.value.models
    .split(',')
    .map((model) => model.trim())
    .filter(Boolean)
)

watch(open, (isOpen) => {
  if (!isOpen) addingKey.value = false
})

watch(secret, (value) => {
  if (value.trim()) addingKey.value = true
})

function toggleAddingKey() {
  addingKey.value = !addingKey.value
  if (!addingKey.value) secret.value = ''
}

function removeSelectedModel(model: string) {
  form.value.models = selectedModels.value.filter((item) => item !== model).join(', ')
}

function endpointBaseUrlLabel(protocol: EndpointProtocol) {
  if (protocol === 'openai') return t('openAiBaseUrl')
  if (protocol === 'anthropic') return t('anthropicBaseUrl')
  return t('baseUrl')
}

function maskedKey(key: ChannelKey) {
  const prefix = key.key_prefix.trim()
  if (!prefix) return '********'
  return `${prefix}${'********'}`
}

function keyHealthType(key: ChannelKey) {
  if (!key.enabled) return 'info'
  return key.healthy ? 'success' : 'danger'
}

function keyHealthLabel(key: ChannelKey) {
  if (!key.enabled) return t('disabled')
  return key.healthy ? t('healthy') : t('unhealthy')
}
</script>

<template>
  <el-dialog
    v-model="open"
    class="channel-dialog"
    :title="mode === 'create' ? t('createChannel') : t('editChannel')"
    width="680px"
  >
    <el-form class="channel-form" label-position="top" @submit.prevent="emit('submit')">
      <el-form-item class="provider-field" :label="t('provider')">
        <el-select
          v-if="mode === 'create'"
          v-model="form.provider"
          class="provider-select"
          filterable
          popper-class="provider-select-dropdown"
          @change="emit('selectProvider', form.provider)"
        >
          <template #prefix>
            <ProviderIcon :provider="form.provider" />
          </template>
          <el-option
            v-for="provider in providerOptions"
            :key="provider.value"
            :label="provider.label"
            :value="provider.value"
          >
            <span class="provider-option">
              <ProviderIcon :provider="provider.value" />
              <span class="provider-option-label">{{ provider.label }}</span>
            </span>
          </el-option>
        </el-select>

        <el-select
          v-else
          :model-value="providerValue"
          class="provider-select"
          disabled
          popper-class="provider-select-dropdown"
        >
          <template #prefix>
            <ProviderIcon :provider="providerValue" />
          </template>
          <el-option
            v-for="provider in providerOptions"
            :key="provider.value"
            :label="provider.label"
            :value="provider.value"
          >
            <span class="provider-option">
              <ProviderIcon :provider="provider.value" />
              <span class="provider-option-label">{{ provider.label }}</span>
            </span>
          </el-option>
        </el-select>
      </el-form-item>

      <el-form-item class="name-field" :label="t('name')">
        <el-input v-model="form.name" :placeholder="t('channelNamePlaceholder')" />
      </el-form-item>

      <div v-if="visibleEndpointRows.length > 1" class="base-url-grid">
        <el-form-item
          v-for="endpoint in visibleEndpointRows"
          :key="endpoint.protocol"
          :label="endpoint.label"
        >
          <el-input
            v-model="form.endpoints[endpoint.protocol].base_url"
            :placeholder="endpoint.placeholder"
          />
        </el-form-item>
      </div>

      <el-form-item v-else :label="t('baseUrl')">
        <el-input v-model="baseUrl" class="base-url-input" :placeholder="t('baseUrlPlaceholder')" />
      </el-form-item>

      <div
        v-if="form.provider === 'openai' && !hideCredentialFilesToggle"
        class="credential-source"
      >
        <label class="credential-source-toggle">
          <span>{{ t('useCredentialFiles') }}</span>
          <el-switch v-model="form.use_credentials" />
        </label>
        <p class="credential-source-hint">
          {{
            form.use_credentials
              ? t('credentialFilesEnabledHint')
              : t('credentialFilesDisabledHint')
          }}
        </p>
      </div>

      <el-form-item v-if="!form.use_credentials" class="api-key-field" :label="t('apiKeyOrJson')">
        <div v-if="showExistingKeyTable" class="existing-keys">
          <el-table :data="existingKeys" class="existing-keys-table" size="small" row-key="id">
            <el-table-column :label="t('upstreamApiKey')" min-width="0">
              <template #default="{ row }: { row: ChannelKey }">
                <code class="existing-key-value">{{ maskedKey(row) }}</code>
              </template>
            </el-table-column>
            <el-table-column :label="t('status')" width="76">
              <template #default="{ row }: { row: ChannelKey }">
                <el-tag :type="keyHealthType(row)" effect="light" round>
                  {{ keyHealthLabel(row) }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column :label="t('lastUsedAt')" width="106">
              <template #default="{ row }: { row: ChannelKey }">
                <span class="existing-key-time">{{ formatCompactDateTime(row.last_used_at) }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="72" align="center" header-align="center">
              <template #default="{ row }: { row: ChannelKey }">
                <div class="existing-key-actions">
                  <el-button
                    text
                    :loading="copyingKeyId === row.id"
                    :aria-label="t('copyKey')"
                    @click="emit('copyKey', row)"
                  >
                    <el-icon><CopyDocument /></el-icon>
                  </el-button>
                  <el-button
                    text
                    type="danger"
                    :loading="deletingKeyId === row.id"
                    :aria-label="t('delete')"
                    @click="emit('deleteKey', row)"
                  >
                    <el-icon><Delete /></el-icon>
                  </el-button>
                </div>
              </template>
            </el-table-column>
          </el-table>
          <div class="add-key-panel" :class="{ 'is-open': addingKey }">
            <button class="add-key-toggle" type="button" @click="toggleAddingKey">
              <el-icon><Plus /></el-icon>
              <span>{{ t('addUpstreamKey') }}</span>
            </button>
            <el-input
              v-if="addingKey"
              v-model="secret"
              class="secret-input is-inline-add"
              :rows="2"
              type="textarea"
              :placeholder="secretPlaceholder"
            />
          </div>
        </div>
        <el-input
          v-else
          v-model="secret"
          class="secret-input"
          :rows="2"
          type="textarea"
          :placeholder="secretPlaceholder"
        />
      </el-form-item>

      <el-form-item :label="t('models')">
        <div class="model-summary-field" :class="{ 'is-empty': selectedModels.length === 0 }">
          <div v-if="selectedModels.length" class="model-summary-content">
            <div class="model-tags">
              <span v-for="model in selectedModels" :key="model" class="model-tag">
                <span class="model-tag-text">{{ model }}</span>
                <button
                  class="model-tag-remove"
                  type="button"
                  :aria-label="t('removeModel')"
                  @click.stop="removeSelectedModel(model)"
                >
                  <el-icon><Close /></el-icon>
                </button>
              </span>
            </div>
          </div>
          <button
            class="model-summary-action add-key-toggle"
            :class="{ 'is-loading': fetchingModels }"
            type="button"
            :disabled="fetchingModels"
            @click="emit('fetchModels')"
          >
            <el-icon v-if="!fetchingModels"><Plus /></el-icon>
            {{ fetchingModels ? t('fetchingModels') : t('addModel') }}
          </button>
        </div>
      </el-form-item>

      <button class="hidden-submit" type="submit" />
    </el-form>

    <template #footer>
      <div class="dialog-footer">
        <el-button @click="open = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="submitting" @click="emit('submit')">
          {{ mode === 'create' ? t('create') : t('save') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.channel-form {
  display: grid;
  gap: 13px;
}

.provider-field {
  margin-bottom: 0;
  width: 50%;
}

.name-field {
  width: 50%;
}

.provider-select {
  width: 100%;
}

.provider-select :deep(.el-select__prefix) {
  left: 12px;
}

.provider-select :deep(.el-select__wrapper) {
  gap: 5px;
}

.provider-select :deep(.el-select__placeholder) {
  padding-left: 2px;
}

.provider-option {
  align-items: center;
  display: flex;
  gap: 5px;
  min-width: 0;
}

.provider-option-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

:global(.provider-select-dropdown .el-select-dropdown__item) {
  height: 42px;
  line-height: 42px;
  padding: 0 14px;
}

:global(.provider-select-dropdown .el-select-dropdown__item.selected .provider-icon) {
  border-color: currentColor;
}

.base-url-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr);
}

.model-summary-field {
  align-items: stretch;
  border: 1px solid #e3e8ef;
  border-radius: 8px;
  display: grid;
  gap: 0;
  min-height: 54px;
  padding: 0;
  width: 100%;
}

.model-summary-field.is-empty {
  min-height: 0;
}

.model-tags {
  display: flex;
  flex: 1;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
  padding: 10px 12px 10px 14px;
}

.model-summary-content {
  min-width: 0;
}

.model-tag {
  align-items: center;
  background: #ffffff;
  border: 1px solid #d8e4ff;
  border-radius: 999px;
  color: #49617f;
  display: inline-flex;
  font-size: 12px;
  font-weight: 650;
  gap: 5px;
  height: 24px;
  line-height: 16px;
  max-width: 100%;
  overflow: hidden;
  padding: 0 6px 0 9px;
}

.model-tag-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-tag-remove {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  border-radius: 999px;
  color: #8a9ab3;
  cursor: pointer;
  display: inline-flex;
  flex: 0 0 auto;
  height: 18px;
  justify-content: center;
  padding: 0;
  width: 18px;
}

.model-tag-remove:hover {
  background: #eef4ff;
  color: #3156b3;
}

.model-tag-remove :deep(.el-icon) {
  font-size: 12px;
}

.model-summary-action {
  border-top: 1px solid #edf1f6;
}

.model-summary-action.is-loading::before {
  animation: fetch-spin 0.8s linear infinite;
  border: 2px solid #c7d7fe;
  border-top-color: var(--brand-blue);
  border-radius: 999px;
  content: '';
  height: 13px;
  width: 13px;
}

.model-summary-action:disabled {
  color: #98a2b3;
  cursor: default;
}

@keyframes fetch-spin {
  to {
    transform: rotate(360deg);
  }
}

.credential-source {
  display: grid;
  gap: 2px;
}

.credential-source-toggle {
  align-items: center;
  color: #334155;
  display: flex;
  font-size: 14px;
  font-weight: 700;
  justify-content: space-between;
}

.credential-source-hint {
  color: #64748b;
  font-size: 13px;
  line-height: 1.55;
  margin: 0;
}

.api-key-field {
  margin-bottom: 0;
}

.api-key-field :deep(.el-form-item__content) {
  display: grid;
  gap: 10px;
}

.existing-keys {
  background: #ffffff;
  border: 1px solid #e3e8ef;
  border-radius: 7px;
  overflow: hidden;
  width: 100%;
}

.existing-keys-table {
  border: 0;
  border-radius: 0;
  width: 100%;
}

.existing-keys-table :deep(.el-table__inner-wrapper::before) {
  background: #edf1f6;
  height: 1px;
}

.existing-keys-table :deep(.el-table__header th) {
  background: #f8fafc;
  color: #475569;
  font-size: 12px;
  font-weight: 720;
}

.existing-keys-table :deep(.el-table__cell) {
  padding: 9px 0;
}

.existing-keys-table :deep(.cell) {
  padding: 0 8px;
}

.existing-keys-table :deep(.el-table__body-wrapper) {
  max-height: 214px;
  overflow-y: auto;
}

.existing-keys-table :deep(.el-button) {
  height: 26px;
  min-height: 26px;
  padding: 0 4px;
}

.existing-key-value {
  color: #475569;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
  line-height: 1;
  min-width: 0;
  overflow: hidden;
  padding: 0;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.existing-key-time {
  color: #64748b;
  font-size: 12px;
  white-space: nowrap;
}

.existing-key-actions {
  align-items: center;
  display: inline-flex;
  gap: 0;
  justify-content: center;
}

.add-key-panel {
  display: grid;
  gap: 0;
}

.add-key-toggle {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  color: #667085;
  cursor: pointer;
  display: flex;
  font: inherit;
  font-size: 13px;
  font-weight: 640;
  gap: 6px;
  min-height: 42px;
  padding: 0 18px;
  text-align: left;
  width: 100%;
}

.add-key-toggle:hover {
  background: transparent;
  color: var(--brand-blue);
}

.add-key-panel.is-open .add-key-toggle {
  border-bottom: 1px solid #edf1f6;
  color: #334155;
}

.secret-input.is-inline-add :deep(.el-textarea__inner) {
  background: transparent;
  border: 0;
  border-radius: 0;
  box-shadow: none;
  min-height: 76px;
  padding: 12px 18px;
}

.secret-input.is-inline-add :deep(.el-textarea__inner:focus) {
  box-shadow: none;
}

.secret-input :deep(.el-textarea__inner) {
  overflow-wrap: break-word;
  word-break: normal;
  white-space: pre-wrap;
}

.dialog-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

:global(.channel-dialog) {
  border-radius: 8px;
  max-width: calc(100vw - 32px);
}

:global(.channel-dialog .el-dialog__header) {
  margin: 0;
  padding: 18px 22px 14px;
}

:global(.channel-dialog .el-dialog__title) {
  color: #111827;
  font-size: 18px;
  font-weight: 760;
  line-height: 1.2;
}

:global(.channel-dialog .el-dialog__headerbtn) {
  right: 12px;
  top: 10px;
}

:global(.channel-dialog .el-dialog__body) {
  padding: 18px 22px;
}

:global(.channel-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 22px 18px;
}

.channel-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.channel-form :deep(.el-form-item__label) {
  color: #475569;
  font-size: 13px;
  font-weight: 680;
  line-height: 1.25;
  margin-bottom: 7px;
}

.provider-field :deep(.el-form-item__content),
.name-field :deep(.el-form-item__content) {
  width: 100%;
}

.channel-form :deep(.el-input__wrapper),
.channel-form :deep(.el-select__wrapper) {
  border-radius: 7px;
  min-height: 38px;
}

.channel-form :deep(.el-input__inner) {
  font-size: 14px;
}

.channel-form :deep(.el-textarea__inner) {
  border-radius: 7px;
  font-size: 14px;
  min-height: 108px;
  padding: 12px 14px;
}

.dialog-footer :deep(.el-button) {
  border-radius: 7px;
  font-weight: 680;
  min-height: 34px;
  min-width: 86px;
}

.hidden-submit {
  display: none;
}

@media (max-width: 760px) {
  .provider-field,
  .name-field {
    width: 100%;
  }

  .model-summary-field {
    align-items: flex-start;
    display: grid;
    gap: 8px;
    grid-template-columns: 1fr;
  }

  .model-summary-content {
    align-items: flex-start;
    flex-direction: column;
  }

  .model-tags {
    width: 100%;
  }

  :global(.channel-dialog .el-dialog__header) {
    padding: 24px 22px 10px;
  }

  :global(.channel-dialog .el-dialog__body) {
    padding: 16px 22px 18px;
  }

  :global(.channel-dialog .el-dialog__footer) {
    padding: 16px 22px 22px;
  }
}
</style>
