<script setup lang="ts">
import ProviderIcon from '../../ProviderIcon.vue'
import { useLocale } from '../../../composables/useLocale'
import type { ChannelForm } from '../../../composables/useChannels'
import type { ChannelProviderOption } from '../../../utils/channel'

const open = defineModel<boolean>('open', { required: true })
const form = defineModel<ChannelForm>('form', { required: true })
const baseUrl = defineModel<string>('baseUrl', { required: true })
const secret = defineModel<string>('secret', { required: true })

defineProps<{
  mode: 'create' | 'edit'
  providerOptions: ChannelProviderOption[]
  providerValue: string
  baseUrlReadonly: boolean
  fetchingModels: boolean
  submitting: boolean
  modelsInputPlaceholder: string
  modelsInputReadonly: boolean
  secretPlaceholder: string
}>()

const emit = defineEmits<{
  fetchModels: []
  selectProvider: [provider: string]
  submit: []
}>()

const { t } = useLocale()
</script>

<template>
  <el-dialog
    v-model="open"
    class="channel-dialog"
    :title="mode === 'create' ? t('createChannel') : t('editChannel')"
    width="620px"
  >
    <el-form class="channel-form" label-position="top" @submit.prevent="emit('submit')">
      <div class="provider-row">
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

        <label class="status-toggle">
          <span>{{ t('status') }}</span>
          <el-switch v-model="form.enabled" />
        </label>
      </div>

      <el-form-item :label="t('name')">
        <el-input v-model="form.name" :placeholder="t('channelNamePlaceholder')" />
      </el-form-item>

      <div
        v-if="form.provider === 'custom' || form.provider === 'newapi'"
        class="manual-base-url-grid"
      >
        <el-form-item :label="t('openAiBaseUrl')">
          <el-input
            v-model="form.endpoints.openai.base_url"
            :placeholder="t('baseUrlPlaceholder')"
          />
        </el-form-item>
        <el-form-item :label="t('anthropicBaseUrl')">
          <el-input
            v-model="form.endpoints.anthropic.base_url"
            :placeholder="t('anthropicBaseUrlPlaceholder')"
          />
        </el-form-item>
      </div>

      <el-form-item v-else :label="t('baseUrl')">
        <el-input
          v-model="baseUrl"
          class="base-url-input"
          :class="{ 'is-readonly': baseUrlReadonly }"
          :placeholder="t('baseUrlPlaceholder')"
          :readonly="baseUrlReadonly"
        />
      </el-form-item>

      <div v-if="form.provider === 'openai'" class="credential-source">
        <label class="credential-source-toggle">
          <span>{{ t('useCredentialFiles') }}</span>
          <el-switch v-model="form.use_credentials" />
        </label>
        <p class="credential-source-hint">
          {{
            form.use_credentials ? t('credentialFilesEnabledHint') : t('credentialFilesDisabledHint')
          }}
        </p>
      </div>

      <el-form-item v-if="!form.use_credentials" class="api-key-field" :label="t('apiKeyOrJson')">
        <el-input
          v-model="secret"
          class="secret-input"
          :rows="2"
          type="textarea"
          :placeholder="secretPlaceholder"
        />
      </el-form-item>

      <el-form-item :label="t('models')">
        <div class="models-row">
          <el-input
            v-model="form.models"
            :placeholder="modelsInputPlaceholder"
            :readonly="modelsInputReadonly"
          />
          <button
            class="auto-fetch-link"
            :class="{ 'is-loading': fetchingModels }"
            type="button"
            :disabled="fetchingModels"
            @click="emit('fetchModels')"
          >
            {{ fetchingModels ? t('fetchingModels') : t('autoFetch') }}
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

.provider-row {
  align-items: end;
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr) 86px;
}

.provider-field {
  margin-bottom: 0;
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

.status-toggle {
  align-items: center;
  align-self: end;
  color: #475569;
  display: flex;
  font-size: 14px;
  font-weight: 720;
  gap: 8px;
  justify-content: flex-end;
  min-height: 42px;
}

.base-url-input.is-readonly :deep(.el-input__wrapper) {
  background: #f8fafc;
  box-shadow: 0 0 0 1px #e3e8ef inset;
}

.base-url-input.is-readonly :deep(.el-input__inner) {
  color: #667085;
  -webkit-text-fill-color: #667085;
}

.manual-base-url-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr);
}

.models-row {
  align-items: center;
  display: grid;
  gap: 10px;
  grid-template-columns: minmax(0, 1fr) auto;
  width: 100%;
}

.auto-fetch-link {
  align-items: center;
  appearance: none;
  background: transparent;
  border: 0;
  color: var(--brand-blue);
  cursor: pointer;
  display: inline-flex;
  font: inherit;
  font-size: 14px;
  font-weight: 740;
  gap: 6px;
  min-height: 42px;
  padding: 0 2px;
  text-decoration: underline;
  text-underline-offset: 3px;
  white-space: nowrap;
}

.auto-fetch-link:disabled {
  color: #98a2b3;
  cursor: default;
}

.auto-fetch-link.is-loading::before {
  animation: fetch-spin 0.8s linear infinite;
  border: 2px solid #c7d7fe;
  border-top-color: var(--brand-blue);
  border-radius: 999px;
  content: '';
  height: 13px;
  width: 13px;
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
  .provider-row {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .status-toggle {
    justify-content: space-between;
    min-height: 32px;
    padding-bottom: 0;
  }

  .models-row {
    align-items: stretch;
    grid-template-columns: 1fr;
  }

  .auto-fetch-link {
    justify-self: start;
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
