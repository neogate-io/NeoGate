<script setup lang="ts">
import { ElMessage } from 'element-plus/es/components/message/index'
import { Lock } from '@element-plus/icons-vue'
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { resetPassword } from '../../api/auth'
import LocaleToggleButton from '../../components/common/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { ApiError, readError } from '../../utils/errors'

const route = useRoute()
const router = useRouter()
const { t } = useLocale()
const password = ref('')
const error = ref('')
const submitting = ref(false)
const minPasswordLength = 8

async function submitReset() {
  error.value = ''
  const token = typeof route.query.token === 'string' ? route.query.token : ''
  if (!token) {
    error.value = t('passwordResetInvalid')
    return
  }
  if (!password.value) {
    error.value = t('passwordRequired')
    return
  }
  if (password.value.length < minPasswordLength) {
    error.value = t('passwordMinLength')
    return
  }

  await withLoading(submitting, async () => {
    try {
      await resetPassword(token, password.value)
      ElMessage.success(t('passwordResetSuccess'))
      await router.replace({ name: 'login' })
    } catch (err) {
      error.value = readResetError(err)
    }
  })
}

function readResetError(err: unknown) {
  if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
    return t('passwordResetInvalid')
  }
  if (err instanceof ApiError && err.code === 'password_min_length') {
    return t('passwordMinLength')
  }
  return readError(err)
}
</script>

<template>
  <main class="login-shell">
    <LocaleToggleButton class="login-language home-language-button" />
    <section class="login-stage">
      <el-form class="login-panel" @submit.prevent="submitReset">
        <div class="login-panel-heading">
          <h1>{{ t('resetPasswordTitle') }}</h1>
          <p>{{ t('resetPasswordHint') }}</p>
        </div>
        <div class="login-fields">
          <label class="login-field">
            <span>{{ t('newPassword') }}</span>
            <el-input
              v-model="password"
              :prefix-icon="Lock"
              :placeholder="t('newPassword')"
              show-password
              size="large"
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
          {{ t('resetPassword') }}
        </el-button>
        <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" />
      </el-form>
    </section>
  </main>
</template>
