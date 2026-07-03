<script setup lang="ts">
import { Link, View } from '@element-plus/icons-vue'
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

function submitCreate() {
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
    :title="
      mode === 'edit'
        ? `编辑${create.typeLabel(create.form.appType)}`
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
        <strong>{{ item.label }}</strong>
        <span>{{ item.enabled ? item.description : '即将支持' }}</span>
      </button>
    </div>

    <el-form
      v-else-if="create.form.step === 2 || mode === 'edit'"
      class="app-create-form"
      label-position="top"
      @submit.prevent="submitCreate"
    >
      <div class="app-form-grid">
        <el-form-item class="app-name-field" label="应用名称">
          <el-input v-model="create.form.name" placeholder="例如 研发知识助手" />
        </el-form-item>
      </div>

      <el-form-item label="描述（可选）">
        <el-input
          v-model="create.form.description"
          placeholder="简单说明这个应用的用途，例如：回答研发制度、流程和常见问题"
          type="textarea"
          :rows="2"
        />
      </el-form-item>

      <div class="app-form-divider">大模型设置</div>

      <div class="app-form-grid">
        <el-form-item label="默认模型">
          <el-select v-model="create.form.model" filterable placeholder="选择可用模型">
            <el-option
              v-for="item in create.modelOptions.value"
              :key="item.model"
              :label="item.model"
              :value="item.model"
            >
              <div class="app-model-option">
                <span>{{ item.model }}</span>
                <small>{{ item.channel_count }} 个渠道</small>
              </div>
            </el-option>
            <template #empty>
              <span class="app-model-empty">暂无可用模型，请先配置渠道模型</span>
            </template>
          </el-select>
        </el-form-item>
      </div>

      <el-form-item v-if="mode !== 'edit'" label="使用场景">
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
            <strong>{{ item.label }}</strong>
            <span>{{ item.description }}</span>
            <el-button
              v-if="create.form.usageScenario === item.value && create.canApplyScenarioPrompt.value"
              class="scenario-prompt-button"
              link
              type="primary"
              @click.stop="create.applySelectedScenarioPrompt"
            >
              使用默认提示词
            </el-button>
          </el-radio>
        </el-radio-group>
      </el-form-item>

      <el-form-item label="系统提示词">
        <el-input v-model="create.form.systemPrompt" type="textarea" :rows="3" />
      </el-form-item>

      <el-collapse class="app-advanced-collapse">
        <el-collapse-item title="高级设置" name="advanced">
          <div class="app-form-grid">
            <el-form-item label="上下文轮数">
              <el-input-number v-model="create.form.contextTurns" :min="0" :max="50" />
            </el-form-item>
            <el-form-item label="最大输出 Token">
              <el-input-number v-model="create.form.maxOutputTokens" :min="1" :max="128000" />
            </el-form-item>
          </div>
        </el-collapse-item>
      </el-collapse>

      <div class="app-form-divider">接入配置</div>

      <template v-if="create.form.appType === 'wecom'">
        <div class="app-form-grid">
          <el-form-item label="企业ID（CorpID）">
            <el-input
              v-model="create.form.corpId"
              placeholder="在企业微信管理后台 > 我的企业中查看"
            />
          </el-form-item>
          <el-form-item label="应用 AgentID">
            <el-input v-model="create.form.agentId" placeholder="在自建应用详情中查看" />
          </el-form-item>
          <el-form-item label="应用 Secret">
            <el-input
              v-model="create.form.corpSecret"
              placeholder="在自建应用详情中查看"
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item label="回调 Token">
            <el-input
              v-model="create.form.callbackToken"
              placeholder="与企业微信回调配置中的 Token 保持一致"
              show-password
              type="password"
            />
          </el-form-item>
        </div>
        <el-form-item label="EncodingAESKey">
          <el-input
            v-model="create.form.encodingAesKey"
            maxlength="43"
            placeholder="从企业微信回调配置中复制，必须是 43 位"
            show-word-limit
            show-password
            type="password"
          />
          <p class="app-form-hint">
            企业微信校验 URL 时会使用这个密钥加密 echostr，NeoGate 保存的值必须和企业微信后台一致。
          </p>
        </el-form-item>
      </template>

      <template v-if="create.form.appType === 'webhook'">
        <el-form-item label="Webhook Secret">
          <el-input
            v-model="create.form.webhookSecret"
            :placeholder="mode === 'edit' ? '留空则保持当前密钥不变' : ''"
            show-password
            type="password"
          />
        </el-form-item>
      </template>

      <template v-if="create.form.appType === 'feishu'">
        <div class="app-form-grid">
          <el-form-item label="App ID">
            <el-input
              v-model="create.form.feishuAppId"
              placeholder="在飞书开发者后台 > 凭证与基础信息中查看"
            />
          </el-form-item>
          <el-form-item label="App Secret">
            <el-input
              v-model="create.form.feishuAppSecret"
              placeholder="在飞书开发者后台 > 凭证与基础信息中查看"
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item label="Verification Token">
            <el-input
              v-model="create.form.feishuVerificationToken"
              placeholder="与飞书事件订阅配置中的 Verification Token 保持一致"
              show-password
              type="password"
            />
          </el-form-item>
          <el-form-item label="Encrypt Key（可选）">
            <el-input
              v-model="create.form.feishuEncryptKey"
              placeholder="开启飞书事件加密时填写"
              show-password
              type="password"
            />
          </el-form-item>
        </div>
        <p class="app-form-hint">
          创建后把事件订阅请求地址复制到飞书开发者后台，并订阅接收消息事件。
        </p>
      </template>

      <template v-if="create.form.appType === 'dingtalk'">
        <el-form-item label="机器人 AppSecret">
          <el-input
            v-model="create.form.dingtalkAppSecret"
            placeholder="填写钉钉机器人基础信息中的 AppSecret"
            show-password
            type="password"
          />
          <p class="app-form-hint">
            创建后把消息接收地址复制到钉钉机器人配置中；NeoGate 会用 AppSecret 校验钉钉回调签名。
          </p>
        </el-form-item>
      </template>

      <template v-if="create.form.appType === 'widget'">
        <el-form-item label="允许嵌入域名（一行一个）">
          <el-input v-model="create.form.allowedDomains" type="textarea" :rows="3" />
        </el-form-item>
        <div class="app-form-grid">
          <el-form-item label="欢迎语">
            <el-input v-model="create.form.welcome" />
          </el-form-item>
          <el-form-item label="主题色">
            <el-color-picker v-model="create.form.themeColor" />
          </el-form-item>
          <el-form-item label="匿名访问">
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
            {{ create.typeLabel(create.createdApp.value?.app_type || create.form.appType) }}已创建
          </span>
        </div>
      </div>

      <el-alert
        v-if="create.createdAccessUrls.value.some((item) => !item.value)"
        type="warning"
        :closable="false"
        show-icon
        title="未生成公开访问 URL，请先配置 PUBLIC_BASE_URL。"
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
            <code>{{ item.value || '未配置公开访问地址' }}</code>
            <el-button
              :disabled="!item.value"
              :icon="Link"
              type="primary"
              @click="emit('copy', item.value)"
            >
              复制 URL
            </el-button>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="app-dialog-footer">
        <el-button v-if="create.form.step === 2 && mode !== 'edit'" @click="create.form.step = 1">
          返回
        </el-button>
        <el-button @click="open = false">
          {{ create.form.step === 3 ? '关闭' : t('cancel') }}
        </el-button>
        <el-button
          v-if="create.form.step === 2 || mode === 'edit'"
          type="primary"
          :loading="mode === 'edit' ? saving : create.saving.value"
          @click="submitCreate"
        >
          {{ mode === 'edit' ? '保存' : '创建应用' }}
        </el-button>
        <el-button
          v-if="create.form.step === 3"
          type="primary"
          :icon="View"
          @click="showCreatedAppDetail"
        >
          查看详情
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
