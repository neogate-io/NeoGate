<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { Connection, Lock, Message, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getSmtpSetting, saveSmtpSetting, testSmtpSetting } from '../../api/settings'
import { useLocale } from '../../composables/useLocale'
import { readError } from '../../utils/errors'

const { t } = useLocale()

const loading = ref(false)
const saving = ref(false)
const testing = ref(false)
const configured = ref(false)
const passwordSet = ref(false)

const form = reactive({
  smtpHost: '',
  smtpPort: 587,
  smtpUsername: '',
  smtpPassword: '',
  smtpTls: true,
  fromEmail: '',
  fromName: '',
  subjectPrefix: ''
})

const passwordStateLabel = computed(() => (passwordSet.value ? t('smtpPasswordSet') : t('smtpPasswordNotSet')))

function applySetting(setting: Awaited<ReturnType<typeof getSmtpSetting>>) {
  configured.value = setting.configured
  form.smtpHost = setting.smtp_host
  form.smtpPort = setting.smtp_port || 587
  form.smtpUsername = setting.smtp_username ?? ''
  form.smtpPassword = ''
  form.smtpTls = setting.smtp_tls
  form.fromEmail = setting.from_email
  form.fromName = setting.from_name ?? ''
  form.subjectPrefix = setting.subject_prefix ?? ''
  passwordSet.value = setting.smtp_password_set
}

async function load() {
  loading.value = true
  try {
    applySetting(await getSmtpSetting())
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function save() {
  saving.value = true
  try {
    const setting = await saveSmtpSetting(smtpPayload())
    applySetting(setting)
    ElMessage.success(t('smtpSettingsSaved'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
  }
}

async function sendTestEmail() {
  testing.value = true
  try {
    await testSmtpSetting(smtpPayload())
    ElMessage.success(t('smtpTestEmailSent'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    testing.value = false
  }
}

function smtpPayload() {
  return {
    smtp_host: form.smtpHost,
    smtp_port: form.smtpPort,
    smtp_username: form.smtpUsername || null,
    smtp_password: form.smtpPassword || null,
    clear_smtp_password: false,
    smtp_tls: form.smtpTls,
    from_email: form.fromEmail,
    from_name: form.fromName || null,
    subject_prefix: form.subjectPrefix || null
  }
}

onMounted(load)
</script>

<template>
  <section v-loading="loading" class="smtp-settings-view">
    <el-form class="smtp-settings-form" label-position="top" @submit.prevent="save">
      <div class="smtp-settings-body">
        <section class="smtp-settings-section">
          <header class="smtp-section-header">
            <el-icon><Connection /></el-icon>
            <h3>{{ t('smtpConnectionSettings') }}</h3>
          </header>

          <div class="smtp-grid smtp-connection-grid">
            <el-form-item class="smtp-host-field" :label="t('smtpHost')">
              <el-input v-model="form.smtpHost" autocomplete="off" :placeholder="t('smtpHostPlaceholder')" />
            </el-form-item>

            <el-form-item :label="t('smtpPort')">
              <el-input-number v-model="form.smtpPort" :min="1" :max="65535" :step="1" />
            </el-form-item>

            <el-form-item class="smtp-switch-field" :label="t('smtpTls')">
              <el-switch v-model="form.smtpTls" />
            </el-form-item>
          </div>
        </section>

        <section class="smtp-settings-section">
          <header class="smtp-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('smtpAuthSettings') }}</h3>
          </header>

          <div class="smtp-grid smtp-auth-grid">
            <el-form-item :label="t('smtpUsername')">
              <el-input v-model="form.smtpUsername" autocomplete="off" :placeholder="t('smtpUsernamePlaceholder')" />
            </el-form-item>

            <el-form-item class="smtp-password-field" :label="t('smtpPassword')">
              <div class="smtp-password-stack">
                <el-input
                  v-model="form.smtpPassword"
                  :prefix-icon="Lock"
                  :placeholder="passwordSet ? passwordStateLabel : t('smtpPasswordPlaceholder')"
                  autocomplete="new-password"
                  show-password
                  type="password"
                />
              </div>
            </el-form-item>
          </div>
        </section>

        <section class="smtp-settings-section">
          <header class="smtp-section-header">
            <el-icon><Message /></el-icon>
            <h3>{{ t('smtpSenderSettings') }}</h3>
          </header>

          <div class="smtp-grid smtp-sender-grid">
            <el-form-item :label="t('mailFromEmail')">
              <el-input v-model="form.fromEmail" autocomplete="off" :placeholder="t('mailFromEmailPlaceholder')" type="email" />
            </el-form-item>

            <el-form-item :label="t('mailFromName')">
              <el-input v-model="form.fromName" autocomplete="off" :placeholder="t('mailFromNamePlaceholder')" />
            </el-form-item>

            <el-form-item class="smtp-subject-field" :label="t('mailSubjectPrefix')">
              <el-input v-model="form.subjectPrefix" autocomplete="off" :placeholder="t('mailSubjectPrefixPlaceholder')" />
            </el-form-item>
          </div>
        </section>

        <div class="settings-actions">
          <el-button class="admin-action-button" :icon="Message" :loading="testing" @click="sendTestEmail">
            {{ t('sendSmtpTestEmail') }}
          </el-button>
          <el-button class="admin-action-button" native-type="submit" type="primary" :icon="Select" :loading="saving">
            {{ t('save') }}
          </el-button>
        </div>
      </div>
    </el-form>
  </section>
</template>

<style scoped>
.smtp-settings-view {
  display: flex;
  justify-content: flex-start;
  width: 100%;
}

.smtp-settings-form {
  background: #fff;
  border: 1px solid #e2e7ef;
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.03);
  overflow: hidden;
  width: min(860px, 100%);
}

.smtp-settings-body {
  display: grid;
  gap: 0;
  padding: 4px 18px 18px;
}

.smtp-settings-section {
  border-bottom: 1px solid #edf1f5;
  display: grid;
  gap: 14px;
  padding: 18px 0 20px;
}

.smtp-settings-section:last-child {
  border-bottom: 0;
  padding-bottom: 0;
}

.smtp-section-header {
  align-items: center;
  color: #202b3c;
  display: grid;
  gap: 9px;
  grid-template-columns: auto minmax(0, 1fr);
}

.smtp-section-header .el-icon {
  color: var(--brand-blue);
  font-size: 17px;
}

.smtp-section-header h3 {
  font-size: 15px;
  font-weight: 760;
  line-height: 1.25;
  margin: 0;
}

.smtp-grid {
  display: grid;
  gap: 16px;
  justify-content: start;
}

.smtp-connection-grid {
  align-items: end;
  grid-template-columns: minmax(280px, 420px) 132px 108px;
}

.smtp-auth-grid {
  align-items: start;
  grid-template-columns: minmax(240px, 340px) minmax(260px, 360px);
}

.smtp-sender-grid {
  grid-template-columns: minmax(240px, 340px) minmax(180px, 220px);
}

.smtp-subject-field {
  max-width: 340px;
}

.smtp-settings-form :deep(.el-input-number),
.smtp-settings-form :deep(.el-select) {
  width: 100%;
}

.smtp-switch-field :deep(.el-form-item__content) {
  width: max-content;
}

.smtp-settings-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.smtp-settings-form :deep(.el-form-item__label) {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 720;
  line-height: 1.2;
  margin-bottom: 8px;
}

.smtp-settings-form :deep(.el-input__wrapper),
.smtp-settings-form :deep(.el-input-number .el-input__wrapper) {
  border-radius: 7px;
  min-height: 34px;
}

.smtp-switch-field :deep(.el-form-item__content) {
  align-items: center;
  min-height: 34px;
}

.smtp-password-stack {
  display: grid;
  gap: 10px;
  width: 100%;
}

.settings-actions {
  border-top: 1px solid #edf1f5;
  display: flex;
  gap: 10px;
  justify-content: flex-end;
  margin-left: auto;
  margin-top: 18px;
  min-width: max-content;
  padding-top: 18px;
}

@media (max-width: 980px) {
  .smtp-settings-form {
    width: 100%;
  }

  .smtp-connection-grid,
  .smtp-auth-grid,
  .smtp-sender-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .smtp-host-field,
  .smtp-password-field,
  .smtp-subject-field {
    grid-column: 1 / -1;
    max-width: none;
  }
}

@media (max-width: 640px) {
  .settings-actions {
    justify-content: stretch;
    margin-left: 0;
    min-width: 0;
  }

  .settings-actions .el-button {
    flex: 1 1 0;
    min-width: 0;
  }

  .smtp-connection-grid,
  .smtp-auth-grid,
  .smtp-sender-grid {
    grid-template-columns: 1fr;
  }

  .smtp-host-field,
  .smtp-password-field {
    grid-column: auto;
  }
}
</style>
