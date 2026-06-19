<script setup lang="ts">
import { Key, Lock } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { updateUserPassword } from '../../api/userPassword'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { useAuthStore } from '../../stores/auth'
import { readPasswordChangeError, readPasswordChangeValidationError } from '../../utils/password'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const { t } = useLocale()
const submitting = ref(false)
const error = ref('')

const form = reactive({
  currentPassword: '',
  newPassword: '',
  confirmPassword: ''
})

function validateForm() {
  error.value = readPasswordChangeValidationError(form, t)
  return !error.value
}

async function submitChange() {
  error.value = ''
  if (!validateForm()) return

  await withLoading(submitting, async () => {
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
    }
  })
}

function readChangeError(err: unknown) {
  return readPasswordChangeError(err, t, {
    sameAsCurrentKey: 'passwordSameAsCurrent'
  })
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
