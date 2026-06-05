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
  <section class="admin-password-view">
    <el-form class="admin-password-form" label-position="top" @submit.prevent="save">
      <div class="admin-password-body">
        <section class="admin-password-section">
          <header class="admin-password-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('adminPasswordSection') }}</h3>
          </header>

          <div class="admin-password-grid">
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

        <div class="admin-password-actions">
          <el-button class="admin-action-button" native-type="submit" type="primary" :icon="Select" :loading="saving">
            {{ t('save') }}
          </el-button>
        </div>
      </div>
    </el-form>
  </section>
</template>

<style scoped>
.admin-password-view {
  display: flex;
  justify-content: flex-start;
  width: 100%;
}

.admin-password-form {
  background: #fff;
  border: 1px solid #e2e7ef;
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.03);
  overflow: hidden;
  width: min(620px, 100%);
}

.admin-password-body {
  display: grid;
  padding: 4px 18px 18px;
}

.admin-password-section {
  border-bottom: 1px solid #edf1f5;
  display: grid;
  gap: 14px;
  padding: 18px 0 20px;
}

.admin-password-section-header {
  align-items: center;
  color: #202b3c;
  display: grid;
  gap: 9px;
  grid-template-columns: auto minmax(0, 1fr);
}

.admin-password-section-header .el-icon {
  color: var(--brand-blue);
  font-size: 17px;
}

.admin-password-section-header h3 {
  font-size: 15px;
  font-weight: 760;
  line-height: 1.25;
  margin: 0;
}

.admin-password-grid {
  display: grid;
  gap: 16px;
  grid-template-columns: minmax(260px, 420px);
}

.admin-password-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.admin-password-form :deep(.el-form-item__label) {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 720;
  line-height: 1.2;
  margin-bottom: 8px;
}

.admin-password-form :deep(.el-input__wrapper) {
  border-radius: 7px;
  min-height: 34px;
}

.admin-password-actions {
  display: flex;
  justify-content: flex-start;
  padding-top: 18px;
}

@media (max-width: 640px) {
  .admin-password-form {
    width: 100%;
  }

  .admin-password-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .admin-password-actions .el-button {
    width: 100%;
  }
}
</style>
