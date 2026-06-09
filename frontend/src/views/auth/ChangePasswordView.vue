<script setup lang="ts">
import { Key, Lock } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { updateUserPassword } from '../../api/userPassword'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { useAuthStore } from '../../stores/auth'
import { ApiError } from '../../utils/errors'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const { t } = useLocale()
const submitting = ref(false)
const error = ref('')
const minPasswordLength = 8

const form = reactive({
  currentPassword: '',
  newPassword: '',
  confirmPassword: ''
})

function validateForm() {
  if (!form.currentPassword || !form.newPassword || !form.confirmPassword) {
    error.value = t('passwordRequired')
    return false
  }
  if (form.newPassword.length < minPasswordLength) {
    error.value = t('passwordMinLength')
    return false
  }
  if (form.newPassword !== form.confirmPassword) {
    error.value = t('passwordMismatch')
    return false
  }
  if (form.currentPassword === form.newPassword) {
    error.value = t('passwordSameAsCurrent')
    return false
  }
  return true
}

async function submitChange() {
  error.value = ''
  if (!validateForm()) return

  submitting.value = true
  try {
    await updateUserPassword({
      current_password: form.currentPassword,
      new_password: form.newPassword
    })
    auth.markPasswordChanged()
    await auth.verifySession(true)
    ElMessage.success(t('passwordChangeSuccess'))
    await router.replace(readRedirectPath())
  } catch (err) {
    error.value = readChangeError(err)
  } finally {
    submitting.value = false
  }
}

function readChangeError(err: unknown) {
  if (err instanceof ApiError && err.message.includes('current password is incorrect')) {
    return t('currentPasswordIncorrect')
  }
  if (err instanceof ApiError && err.message.includes('password must be at least 8 characters')) {
    return t('passwordMinLength')
  }
  if (err instanceof ApiError && err.message.includes('new password cannot be the same')) {
    return t('passwordSameAsCurrent')
  }
  return err instanceof Error ? err.message : String(err)
}

function readRedirectPath() {
  const redirect = route.query.redirect
  return typeof redirect === 'string' && redirect.startsWith('/home') ? redirect : '/home/overview'
}
</script>

<template>
  <main class="login-shell">
    <LocaleToggleButton class="login-language home-language-button" />
    <section class="login-stage">
      <el-form class="login-panel" @submit.prevent="submitChange">
        <div class="login-panel-heading">
          <h1>{{ t('changePasswordTitle') }}</h1>
          <p>{{ t('changePasswordHint') }}</p>
        </div>
        <div class="login-fields">
          <label class="login-field">
            <span>{{ t('currentPassword') }}</span>
            <el-input
              v-model="form.currentPassword"
              :prefix-icon="Key"
              autocomplete="current-password"
              show-password
              size="large"
              type="password"
            />
          </label>
          <label class="login-field">
            <span>{{ t('newPassword') }}</span>
            <el-input
              v-model="form.newPassword"
              :prefix-icon="Lock"
              autocomplete="new-password"
              show-password
              size="large"
              type="password"
            />
          </label>
          <label class="login-field">
            <span>{{ t('confirmNewPassword') }}</span>
            <el-input
              v-model="form.confirmPassword"
              :prefix-icon="Lock"
              autocomplete="new-password"
              show-password
              size="large"
              type="password"
            />
          </label>
        </div>
        <el-button
          class="login-submit"
          native-type="submit"
          type="primary"
          size="large"
          :loading="submitting"
        >
          {{ t('changePassword') }}
        </el-button>
        <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" />
      </el-form>
    </section>
  </main>
</template>
