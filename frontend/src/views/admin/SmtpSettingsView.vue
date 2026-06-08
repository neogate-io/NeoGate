<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
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

const passwordStateLabel = computed(() =>
  passwordSet.value ? t('smtpPasswordSet') : t('smtpPasswordNotSet')
)

const smtpPortInput = computed({
  get: () => (form.smtpPort > 0 ? String(form.smtpPort) : ''),
  set: (value: string) => {
    const digits = value.replace(/\D/g, '').slice(0, 5)
    if (!digits) {
      form.smtpPort = 0
      return
    }
    form.smtpPort = Math.min(Number(digits), 65535)
  }
})

function applySetting(setting: Awaited<ReturnType<typeof getSmtpSetting>>) {
  configured.value = setting.configured
  form.smtpHost = setting.smtp_host
  form.smtpUsername = setting.smtp_username ?? ''
  form.smtpPassword = ''
  form.smtpTls = setting.smtp_tls
  form.smtpPort = setting.smtp_port || 587
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

watch(
  () => form.smtpTls,
  (tls) => {
    form.smtpPort = tls ? 587 : 25
  },
  { flush: 'sync' }
)

onMounted(load)
</script>

<template>
  <section v-loading="loading" class="admin-settings-view">
    <el-form
      class="admin-settings-form smtp-settings-form"
      label-position="top"
      @submit.prevent="save"
    >
      <div class="admin-settings-body">
        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Connection /></el-icon>
            <h3>{{ t('smtpConnectionSettings') }}</h3>
          </header>

          <div class="admin-settings-grid smtp-connection-grid">
            <el-form-item class="smtp-host-field" :label="t('smtpHost')">
              <el-input
                v-model="form.smtpHost"
                autocomplete="off"
                :placeholder="t('smtpHostPlaceholder')"
              />
            </el-form-item>

            <el-form-item :label="t('smtpPort')">
              <el-input
                v-model="smtpPortInput"
                autocomplete="off"
                inputmode="numeric"
                placeholder="587"
              />
            </el-form-item>

            <el-form-item class="admin-settings-switch" :label="t('smtpTls')">
              <el-switch v-model="form.smtpTls" />
            </el-form-item>
          </div>
        </section>

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('smtpAuthSettings') }}</h3>
          </header>

          <div class="admin-settings-grid smtp-auth-grid">
            <el-form-item :label="t('smtpUsername')">
              <el-input
                v-model="form.smtpUsername"
                autocomplete="off"
                :placeholder="t('smtpUsernamePlaceholder')"
              />
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

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Message /></el-icon>
            <h3>{{ t('smtpSenderSettings') }}</h3>
          </header>

          <div class="admin-settings-grid smtp-sender-grid">
            <el-form-item :label="t('mailFromEmail')">
              <el-input
                v-model="form.fromEmail"
                autocomplete="off"
                :placeholder="t('mailFromEmailPlaceholder')"
                type="email"
              />
            </el-form-item>

            <el-form-item :label="t('mailFromName')">
              <el-input
                v-model="form.fromName"
                autocomplete="off"
                :placeholder="t('mailFromNamePlaceholder')"
              />
            </el-form-item>

            <el-form-item class="smtp-subject-field" :label="t('mailSubjectPrefix')">
              <el-input
                v-model="form.subjectPrefix"
                autocomplete="off"
                :placeholder="t('mailSubjectPrefixPlaceholder')"
              />
            </el-form-item>
          </div>
        </section>

        <div class="admin-settings-actions smtp-settings-actions">
          <el-button
            class="admin-action-button"
            :icon="Message"
            :loading="testing"
            @click="sendTestEmail"
          >
            {{ t('sendSmtpTestEmail') }}
          </el-button>
          <el-button
            class="admin-action-button"
            native-type="submit"
            type="primary"
            :icon="Select"
            :loading="saving"
          >
            {{ t('save') }}
          </el-button>
        </div>
      </div>
    </el-form>
  </section>
</template>

<style scoped>
.smtp-settings-form {
  width: min(780px, 100%);
}

.smtp-settings-form :deep(.admin-settings-body) {
  padding: 6px 22px 22px;
}

.smtp-settings-form :deep(.admin-settings-section) {
  gap: 16px;
  padding: 20px 0 22px;
}

.smtp-connection-grid {
  align-items: end;
  column-gap: 18px;
  grid-template-columns: minmax(240px, 320px) 116px 84px;
  row-gap: 14px;
}

.smtp-auth-grid {
  align-items: start;
  column-gap: 32px;
  grid-template-columns: minmax(220px, 300px) minmax(160px, 200px);
  row-gap: 14px;
}

.smtp-sender-grid {
  column-gap: 32px;
  grid-template-columns: minmax(220px, 300px) minmax(160px, 200px);
  row-gap: 14px;
}

.smtp-subject-field {
  max-width: 300px;
}

.smtp-password-stack {
  display: grid;
  gap: 10px;
  width: 100%;
}

.smtp-settings-actions {
  border-top: 0;
  margin-top: 0;
  padding-top: 10px;
}

@media (max-width: 980px) {
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
