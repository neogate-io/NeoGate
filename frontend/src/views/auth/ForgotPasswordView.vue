<script setup lang="ts">
import { ElMessage } from 'element-plus/es/components/message/index'
import { User } from '@element-plus/icons-vue'
import { ref } from 'vue'
import { RouterLink } from 'vue-router'
import { requestPasswordReset } from '../../api/auth'
import LocaleToggleButton from '../../components/common/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { ApiError, isSmtpConfigError } from '../../utils/errors'

const { locale, t } = useLocale()
const email = ref('')
const error = ref('')
const sending = ref(false)

async function sendPasswordReset() {
  error.value = ''
  if (!email.value.trim()) {
    error.value = t('emailRequired')
    return
  }

  await withLoading(sending, async () => {
    try {
      await requestPasswordReset(email.value.trim(), locale.value)
      ElMessage.success(t('passwordResetSent'))
    } catch (err) {
      error.value = readForgotPasswordError(err)
    }
  })
}

function readForgotPasswordError(err: unknown) {
  if (isSmtpConfigError(err)) {
    return t('smtpEmailUnavailable')
  }
  if (err instanceof ApiError && err.status === 400) {
    return t('invalidEmail')
  }
  return err instanceof Error ? err.message : String(err)
}
</script>

<template>
  <main class="login-shell">
    <LocaleToggleButton class="login-language home-language-button" />
    <section class="login-stage">
      <el-form class="login-panel" @submit.prevent="sendPasswordReset">
        <div class="login-panel-heading">
          <h1>{{ t('forgotPasswordTitle') }}</h1>
          <p>{{ t('forgotPasswordHint') }}</p>
        </div>
        <div class="login-fields">
          <label class="login-field">
            <span>{{ t('email') }}</span>
            <el-input
              v-model="email"
              :prefix-icon="User"
              :placeholder="t('loginAccountPlaceholder')"
              type="email"
              size="large"
            />
          </label>
        </div>
        <el-button
          class="login-submit"
          native-type="submit"
          type="primary"
          size="large"
          :loading="sending"
        >
          {{ t('sendPasswordReset') }}
        </el-button>
        <RouterLink class="login-secondary-link" to="/login">{{ t('backToLogin') }}</RouterLink>
        <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" />
      </el-form>
    </section>
  </main>
</template>
