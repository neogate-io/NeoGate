<script setup lang="ts">
import { reactive, ref } from 'vue'
import { Key, Lock, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { updateAdminPassword } from '../../api/settings'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import {
  readPasswordChangeError,
  readPasswordChangeValidationError,
  resetPasswordChangeForm
} from '../../utils/password'

const { t } = useLocale()

const saving = ref(false)

const form = reactive({
  currentPassword: '',
  newPassword: '',
  confirmPassword: ''
})

function resetForm() {
  resetPasswordChangeForm(form)
}

function validateForm() {
  const error = readPasswordChangeValidationError(form, t, {
    mismatchKey: 'adminPasswordMismatch',
    sameAsCurrentKey: 'adminPasswordSame'
  })
  if (error) {
    ElMessage.error(error)
    return false
  }
  return true
}

function passwordError(err: unknown) {
  return readPasswordChangeError(err, t, {
    currentIncorrectKey: 'adminPasswordCurrentIncorrect',
    fallback: 'readError'
  })
}

async function save() {
  if (!validateForm()) return

  await withLoading(saving, async () => {
    try {
      await updateAdminPassword({
        current_password: form.currentPassword,
        new_password: form.newPassword
      })
      resetForm()
      ElMessage.success(t('adminPasswordSaved'))
    } catch (err) {
      ElMessage.error(passwordError(err))
    }
  })
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
