<script setup lang="ts">
import { computed, ref } from 'vue'
import { Link, View } from '@element-plus/icons-vue'
import type { FormInstance, FormItemRule, FormRules } from 'element-plus'
import type { UseAppCreate } from '../../../composables/useAppCreate'
import { useLocale } from '../../../composables/useLocale'
import type { AppRecord } from '../../../types/admin'

const open = defineModel<boolean>('open', { required: true })

const props = defineProps<{
  create: UseAppCreate
  mode?: 'create' | 'edit'
  saving?: boolean
}>()
const create = props.create

const emit = defineEmits<{
  copy: [value?: string | null]
  save: []
  showDetail: [app: AppRecord]
}>()

const { t } = useLocale()
const formRef = ref<FormInstance>()

function requiredRule(message: string, trigger: 'blur' | 'change' = 'blur'): FormItemRule {
  return { required: true, whitespace: true, message, trigger }
}

const formRules = computed<FormRules>(() => {
  const rules: FormRules = {
    name: [requiredRule(t('appNameRequired'))],
    model: [requiredRule(t('appModelRequired'), 'change')]
  }

  if (create.form.appType === 'wecom') {
    rules.corpId = [requiredRule(t('appWecomCorpIdRequired'))]
    rules.agentId = [requiredRule(t('appWecomAgentIdRequired'))]
    if (props.mode !== 'edit') {
      rules.corpSecret = [requiredRule(t('appWecomSecretRequired'))]
      rules.callbackToken = [requiredRule(t('appCallbackTokenRequired'))]
    }
    rules.encodingAesKey = [
      {
        trigger: 'blur',
        validator: (_rule, value: string, callback) => {
          const normalized = value.trim()
          if (!normalized && props.mode === 'edit') return callback()
          if (!normalized) return callback(new Error(t('appAesKeyRequired')))
          if (normalized.length !== 43) return callback(new Error(t('appAesKeyLength')))
          callback()
        }
      }
    ]
  }

  if (create.form.appType === 'feishu') {
    rules.feishuAppId = [requiredRule(t('appFeishuAppIdRequired'))]
    if (props.mode !== 'edit') {
      rules.feishuAppSecret = [requiredRule(t('appFeishuAppSecretRequired'))]
      rules.feishuVerificationToken = [requiredRule(t('appFeishuTokenRequired'))]
    }
  }

  if (create.form.appType === 'dingtalk' && props.mode !== 'edit') {
    rules.dingtalkAppSecret = [requiredRule(t('appDingtalkSecretRequired'))]
  }

  return rules
})

async function submitCreate() {
  const valid = await formRef.value?.validate().catch(() => false)
  if (!valid) return
  if (props.mode === 'edit') {
    emit('save')
    return
  }
  void props.create.submitCreate()
}

function showCreatedAppDetail() {
  if (!props.create.createdApp.value) return
  open.value = false
  emit('showDetail', props.create.createdApp.value)
}
</script>

<template>
  <el-dialog
    v-model="open"
    class="app-dialog"
    :close-on-click-modal="false"
    :title="
      mode === 'edit'
        ? t('appEditTitle', { type: create.typeLabel(create.form.appType) })
        : create.createDialogTitle.value
    "
    width="680px"
  >
    <div v-if="create.form.step === 1 && mode !== 'edit'" class="app-type-grid">
      <button
        v-for="item in create.appTypes"
        :key="item.type"
        class="app-type-card"
        :class="{ 'is-disabled': !item.enabled }"
        type="button"
        @click="create.selectType(item.type, item.enabled)"
      >
        <img class="app-type-card-icon" :src="item.iconUrl" alt="" />
        <strong>{{ t(item.labelKey) }}</strong>
        <span>{{ item.enabled ? t(item.descriptionKey) : t('appTypeComingSoon') }}</span>
      </button>
    </div>

    <el-form
      ref="formRef"
      v-else-if="create.form.step === 2 || mode === 'edit'"
      class="app-create-form"
      label-position="top"
      :model="create.form"
      :rules="formRules"
      @submit.prevent="submitCreate"
    >
      <div class="app-form-grid">
        <el-form-item class="app-name-field" :label="t('appName')" prop="name">
          <el-input v-model="create.form.name" :placeholder="t('appNamePlaceholder')" />
        </el-form-item>
      </div>

      <el-form-item :label="t('appDescriptionOptional')">
        <el-input
          v-model="create.form.description"
          :placeholder="t('appDescriptionPlaceholder')"
          type="textarea"
          :rows="2"
        />
      </el-form-item>

      <div class="app-form-divider">{{ t('appModelSettings') }}</div>

      <div class="app-form-grid">
        <el-form-item :label="t('appDefaultModel')" prop="model">
          <el-select v-model="create.form.model" filterable :placeholder="t('appSelectModel')">
            <el-option
              v-for="item in create.modelOptions.value"
              :key="item.model"
              :label="item.model"
              :value="item.model"
            >
              <div class="app-model-option">
                <span>{{ item.model }}</span>
                <small>{{ t('appChannelCount', { count: item.channel_count }) }}</small>
              </div>
            </el-option>
            <template #empty>
              <span class="app-model-empty">{{ t('appNoModels') }}</span>
            </template>
          </el-select>
        </el-form-item>
      </div>

      <el-form-item v-if="mode !== 'edit'" :label="t('appUsageScenario')">
        <el-radio-group
          v-model="create.form.usageScenario"
          class="usage-scenario-grid"
          @change="create.applyUsageScenario"
        >
          <el-radio
            v-for="item in create.usageScenarios"
            :key="item.value"
            class="usage-scenario-option"
            :value="item.value"
          >
            <strong>{{ t(item.labelKey) }}</strong>
            <span>{{ t(item.descriptionKey) }}</span>
            <el-button
              v-if="create.form.usageScenario === item.value && create.canApplyScenarioPrompt.value"
              class="scenario-prompt-button"
              link
              type="primary"
              @click.stop="create.applySelectedScenarioPrompt"
            >
              {{ t('appUseDefaultPrompt') }}
            </el-button>
          </el-radio>
        </el-radio-group>
      </el-form-item>

      <el-form-item :label="t('appSystemPrompt')">
        <el-input v-model="create.form.systemPrompt" type="textarea" :rows="3" />
      </el-form-item>

      <el-collapse class="app-advanced-collapse">
        <el-collapse-item :title="t('appAdvancedSettings')" name="advanced">
          <div class="app-form-grid">
            <el-form-item :label="t('appContextTurns')">
              <el-input-number v-model="create.form.contextTurns" :min="0" :max="50" />
            </el-form-item>
            <el-form-item :label="t('appMaxOutputTokens')">
              <el-input-number v-model="create.form.maxOutputTokens" :min="1" :max="128000" />
            </el-form-item>
          </div>
        </el-collapse-item>
      </el-collapse>

      <div class="app-form-divider">{{ t('appAccessSettings') }}</div>

      <template v-if="create.form.appType === 'wecom'">
        <div class="app-form-grid">
          <el-form-item :label="t('appWecomCorpId')" prop="corpId">
            <el-input v-model="create.form.corpId" :placeholder="t('appWecomCorpIdPlaceholder')" />
          </el-form-item>
          <el-form-item :label="t('appWecomAgentId')" prop="agentId">
            <el-input
              v-model="create.form.agentId"
              :placeholder="t('appWecomDetailsPlaceholder')"
            />
          </el-form-item>
          <el-form-item :label="t('appWecomSecret')" prop="corpSecret">
            <el-input
              v-model="create.form.corpSecret"
              :placeholder="
                mode === 'edit' ? t('appKeepSecretPlaceholder') : t('appWecomDetailsPlaceholder')
              "
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item :label="t('appCallbackToken')" prop="callbackToken">
            <el-input
              v-model="create.form.callbackToken"
              :placeholder="
                mode === 'edit' ? t('appKeepSecretPlaceholder') : t('appCallbackTokenPlaceholder')
              "
              show-password
              type="password"
            />
          </el-form-item>
        </div>
        <el-form-item label="EncodingAESKey" prop="encodingAesKey">
          <el-input
            v-model="create.form.encodingAesKey"
            maxlength="43"
            :placeholder="
              mode === 'edit' ? t('appKeepSecretPlaceholder') : t('appAesKeyPlaceholder')
            "
            show-word-limit
            show-password
            type="password"
          />
          <p class="app-form-hint">
            {{ t('appAesKeyHelp') }}
          </p>
        </el-form-item>
      </template>

      <template v-if="create.form.appType === 'webhook'">
        <el-form-item label="Webhook Secret">
          <el-input
            v-model="create.form.webhookSecret"
            :placeholder="mode === 'edit' ? t('appKeepSecretPlaceholder') : ''"
            show-password
            type="password"
          />
        </el-form-item>
      </template>

      <template v-if="create.form.appType === 'feishu'">
        <div class="app-form-grid">
          <el-form-item label="App ID" prop="feishuAppId">
            <el-input
              v-model="create.form.feishuAppId"
              :placeholder="t('appFeishuCredentialPlaceholder')"
            />
          </el-form-item>
          <el-form-item label="App Secret" prop="feishuAppSecret">
            <el-input
              v-model="create.form.feishuAppSecret"
              :placeholder="
                mode === 'edit'
                  ? t('appKeepSecretPlaceholder')
                  : t('appFeishuCredentialPlaceholder')
              "
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item label="Verification Token" prop="feishuVerificationToken">
            <el-input
              v-model="create.form.feishuVerificationToken"
              :placeholder="
                mode === 'edit' ? t('appKeepSecretPlaceholder') : t('appFeishuTokenPlaceholder')
              "
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item :label="t('appEncryptKeyOptional')">
            <el-input
              v-model="create.form.feishuEncryptKey"
              :placeholder="
                mode === 'edit' ? t('appKeepSecretPlaceholder') : t('appEncryptKeyPlaceholder')
              "
              show-password
              type="password"
            />
          </el-form-item>
        </div>
        <p class="app-form-hint">
          {{ t('appFeishuHelp') }}
        </p>
      </template>

      <template v-if="create.form.appType === 'dingtalk'">
        <el-form-item :label="t('appDingtalkSecret')" prop="dingtalkAppSecret">
          <el-input
            v-model="create.form.dingtalkAppSecret"
            :placeholder="
              mode === 'edit' ? t('appKeepSecretPlaceholder') : t('appDingtalkSecretPlaceholder')
            "
            show-password
            type="password"
          />
          <p class="app-form-hint">
            {{ t('appDingtalkHelp') }}
          </p>
        </el-form-item>
      </template>

      <template v-if="create.form.appType === 'widget'">
        <el-form-item :label="t('appAllowedDomains')">
          <el-input v-model="create.form.allowedDomains" type="textarea" :rows="3" />
        </el-form-item>
        <div class="app-form-grid">
          <el-form-item :label="t('appWelcomeMessage')">
            <el-input v-model="create.form.welcome" />
          </el-form-item>
          <el-form-item :label="t('appThemeColor')">
            <el-color-picker v-model="create.form.themeColor" />
          </el-form-item>
          <el-form-item :label="t('appAnonymousAccess')">
            <el-switch v-model="create.form.anonymousAccess" />
          </el-form-item>
        </div>
      </template>

      <button class="hidden-submit" type="submit" />
    </el-form>

    <div v-else class="app-create-success">
      <div class="app-create-success-heading">
        <span class="app-type-icon">
          <img
            :src="create.typeMeta(create.createdApp.value?.app_type || create.form.appType).iconUrl"
            alt=""
          />
        </span>
        <div>
          <strong>{{ create.createdApp.value?.name }}</strong>
          <span>
            {{
              t('appCreatedType', {
                type: create.typeLabel(create.createdApp.value?.app_type || create.form.appType)
              })
            }}
          </span>
        </div>
      </div>

      <el-alert
        v-if="create.createdAccessUrls.value.some((item) => !item.value)"
        type="warning"
        :closable="false"
        show-icon
        :title="t('appPublicUrlMissing')"
      />

      <div class="app-access-url-list">
        <div
          v-for="item in create.createdAccessUrls.value"
          :key="item.label"
          class="app-access-url-row"
        >
          <div class="app-access-url-meta">
            <strong>{{ item.label }}</strong>
            <span>{{ item.helper }}</span>
          </div>
          <div class="app-access-url-copy">
            <code>{{ item.value || t('appPublicUrlNotConfigured') }}</code>
            <el-button
              :disabled="!item.value"
              :icon="Link"
              type="primary"
              @click="emit('copy', item.value)"
            >
              {{ t('appCopyUrl') }}
            </el-button>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="app-dialog-footer">
        <el-button v-if="create.form.step === 2 && mode !== 'edit'" @click="create.form.step = 1">
          {{ t('appBack') }}
        </el-button>
        <el-button @click="open = false">
          {{ create.form.step === 3 ? t('appClose') : t('cancel') }}
        </el-button>
        <el-button
          v-if="create.form.step === 2 || mode === 'edit'"
          type="primary"
          :loading="mode === 'edit' ? saving : create.saving.value"
          @click="submitCreate"
        >
          {{ mode === 'edit' ? t('appSave') : t('appCreateAction') }}
        </el-button>
        <el-button
          v-if="create.form.step === 3"
          type="primary"
          :icon="View"
          @click="showCreatedAppDetail"
        >
          {{ t('appViewDetails') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
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

.app-type-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.app-type-card {
  background: #ffffff;
  border: 1px solid var(--admin-border);
  border-radius: 8px;
  cursor: pointer;
  display: grid;
  gap: 8px;
  min-height: 128px;
  padding: 16px;
  text-align: left;
}

.app-type-card-icon {
  display: block;
  height: 28px;
  width: 28px;
}

.app-type-icon img {
  display: block;
  height: 24px;
  width: 24px;
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

.app-form-grid {
  display: grid;
  gap: 13px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.app-model-option {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
  min-width: 0;
}

.app-model-option span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-model-option small {
  color: var(--admin-text-muted);
  flex: none;
  font-size: 12px;
}

.app-model-empty {
  color: var(--admin-text-muted);
  display: block;
  font-size: 13px;
  padding: 10px 12px;
}

.usage-scenario-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(auto-fit, minmax(176px, 1fr));
  width: 100%;
}

.usage-scenario-option.el-radio {
  align-items: flex-start;
  border: 1px solid #d8e0ea;
  border-radius: 7px;
  margin: 0;
  min-height: 72px;
  padding: 12px;
  white-space: normal;
}

.usage-scenario-option.el-radio.is-checked {
  background: var(--admin-primary-soft);
  border-color: #9bbde3;
}

.usage-scenario-option :deep(.el-radio__input) {
  padding-top: 2px;
}

.usage-scenario-option :deep(.el-radio__label) {
  display: grid;
  gap: 4px;
  line-height: 1.35;
  min-width: 0;
  padding-left: 8px;
}

.usage-scenario-option :deep(.el-radio__label strong) {
  color: var(--admin-heading);
  font-size: 13px;
  font-weight: 700;
}

.usage-scenario-option :deep(.el-radio__label span) {
  color: var(--admin-text-muted);
  font-size: 12px;
}

.scenario-prompt-button.el-button {
  --el-button-hover-text-color: var(--admin-primary);
  --el-button-text-color: var(--admin-primary);
  background: #ffffff;
  border: 1px solid #b9d1ec;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 680;
  justify-self: start;
  min-height: 24px;
  padding: 0 10px;
}

.scenario-prompt-button.el-button:hover,
.scenario-prompt-button.el-button:focus-visible {
  background: #f8fbff;
  border-color: var(--admin-primary);
  box-shadow: 0 0 0 2px rgba(23, 107, 175, 0.12);
}

.app-advanced-collapse {
  border-bottom: 0;
  border-top: 0;
  margin-top: -4px;
}

.app-advanced-collapse :deep(.el-collapse-item__header) {
  color: #475569;
  font-size: 13px;
  font-weight: 700;
  height: 38px;
  line-height: 38px;
}

.app-advanced-collapse :deep(.el-collapse-item__wrap) {
  border-bottom: 0;
}

.app-advanced-collapse :deep(.el-collapse-item__content) {
  padding-bottom: 0;
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

.app-form-hint {
  color: var(--admin-text-muted);
  font-size: 12px;
  line-height: 1.5;
  margin: 6px 0 0;
}

.app-dialog-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.app-create-success {
  display: grid;
  gap: 16px;
}

.app-create-success-heading {
  align-items: center;
  background: #f8fafc;
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  display: grid;
  gap: 10px;
  grid-template-columns: auto minmax(0, 1fr);
  padding: 12px;
}

.app-create-success-heading strong {
  color: var(--admin-heading);
  display: block;
  font-size: 15px;
  font-weight: 720;
  line-height: 1.35;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-create-success-heading span {
  color: var(--admin-text-muted);
  display: block;
  font-size: 13px;
  margin-top: 2px;
}

.app-access-url-list {
  display: grid;
  gap: 12px;
}

.app-access-url-row {
  border: 1px solid #d8e0ea;
  border-radius: 8px;
  display: grid;
  gap: 10px;
  padding: 12px;
}

.app-access-url-meta {
  display: grid;
  gap: 4px;
}

.app-access-url-meta strong {
  color: var(--admin-heading);
  font-size: 13px;
  font-weight: 720;
}

.app-access-url-meta span {
  color: var(--admin-text-muted);
  font-size: 12px;
  line-height: 1.5;
}

.app-access-url-copy {
  align-items: stretch;
  display: grid;
  gap: 10px;
  grid-template-columns: minmax(0, 1fr) auto;
}

.app-access-url-copy code {
  align-items: center;
  background: #f8fafc;
  border: 1px solid var(--admin-border-soft);
  border-radius: 7px;
  color: #0f172a;
  display: flex;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
  font-size: 12px;
  line-height: 1.45;
  min-height: 38px;
  min-width: 0;
  overflow-wrap: anywhere;
  padding: 8px 10px;
}

.app-access-url-copy :deep(.el-button) {
  border-radius: 7px;
  font-weight: 680;
  min-height: 38px;
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

:global(.app-dialog .el-dialog__body) {
  padding: 18px 22px;
}

:global(.app-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 22px 18px;
}

.app-create-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.app-create-form :deep(.el-form-item__label) {
  color: #475569;
  font-size: 13px;
  font-weight: 680;
  line-height: 1.25;
  margin-bottom: 7px;
}

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

@media (max-width: 760px) {
  .app-type-grid,
  .app-form-grid {
    grid-template-columns: 1fr;
  }

  .app-access-url-copy {
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
