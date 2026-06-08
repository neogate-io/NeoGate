<script setup lang="ts">
import { reactive, ref } from 'vue'
import { Key, Lock, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { updateAdminPassword } from '../../api/settings'
import { useLocale } from '../../composables/useLocale'
import { ApiError, readError } from '../../utils/errors'

const { t } = useLocale()

const saving = ref(false)

const form = reactive({
  currentPassword: '',
  newPassword: '',
  confirmPassword: ''
})

function resetForm() {
  form.currentPassword = ''
  form.newPassword = ''
  form.confirmPassword = ''
}

function validateForm() {
  if (!form.currentPassword || !form.newPassword || !form.confirmPassword) {
    ElMessage.error(t('passwordRequired'))
    return false
  }
  if (form.newPassword.length < 8) {
    ElMessage.error(t('passwordMinLength'))
    return false
  }
  if (form.newPassword !== form.confirmPassword) {
    ElMessage.error(t('adminPasswordMismatch'))
    return false
  }
  if (form.currentPassword === form.newPassword) {
    ElMessage.error(t('adminPasswordSame'))
    return false
  }
  return true
}

function passwordError(err: unknown) {
  if (err instanceof ApiError && err.message.includes('current password is incorrect')) {
    return t('adminPasswordCurrentIncorrect')
  }
  if (err instanceof ApiError && err.message.includes('password must be at least 8 characters')) {
    return t('passwordMinLength')
  }
  return readError(err)
}

async function save() {
  if (!validateForm()) return

  saving.value = true
  try {
    await updateAdminPassword({
      current_password: form.currentPassword,
      new_password: form.newPassword
    })
    resetForm()
    ElMessage.success(t('adminPasswordSaved'))
  } catch (err) {
    ElMessage.error(passwordError(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="admin-settings-view">
    <el-form
      class="admin-settings-form is-narrow admin-password-form"
      label-position="top"
      @submit.prevent="save"
    >
      <div class="admin-settings-body">
        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('adminPasswordSection') }}</h3>
          </header>

          <div class="admin-settings-grid admin-password-grid">
            <el-form-item :label="t('currentAdminPassword')">
              <el-input
                v-model="form.currentPassword"
                :prefix-icon="Key"
                autocomplete="current-password"
                show-password
                type="password"
              />
            </el-form-item>

            <el-form-item :label="t('newPassword')">
              <el-input
                v-model="form.newPassword"
                :prefix-icon="Lock"
                autocomplete="new-password"
                show-password
                type="password"
              />
            </el-form-item>

            <el-form-item :label="t('confirmNewPassword')">
              <el-input
                v-model="form.confirmPassword"
                :prefix-icon="Lock"
                autocomplete="new-password"
                show-password
                type="password"
              />
            </el-form-item>
          </div>
        </section>

        <div class="admin-settings-actions admin-password-actions">
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
.admin-password-form {
  width: min(520px, 100%);
}

.admin-password-form :deep(.admin-settings-body) {
  padding: 6px 22px 22px;
}

.admin-password-form :deep(.admin-settings-section) {
  border-bottom: 1px solid var(--admin-border-soft);
  gap: 18px;
  padding: 20px 0 16px;
}

.admin-password-grid {
  gap: 14px;
  grid-template-columns: minmax(260px, 360px);
}

.admin-password-actions {
  border-top: 0;
  margin-top: 0;
  padding-top: 12px;
}

@media (max-width: 640px) {
  .admin-password-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
