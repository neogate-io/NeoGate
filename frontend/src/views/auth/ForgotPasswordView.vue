<script setup lang="ts">
import { ElMessage } from 'element-plus/es/components/message/index'
import { User } from '@element-plus/icons-vue'
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { requestPasswordReset } from '../../api/auth'
import { useLocale } from '../../composables/useLocale'
import { ApiError } from '../../utils/errors'

const { locale, t, toggleLocale } = useLocale()
const email = ref('')
const error = ref('')
const sending = ref(false)
const nextLocaleLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中'))

async function sendPasswordReset() {
  error.value = ''
  if (!email.value.trim()) {
    error.value = t('emailRequired')
    return
  }

  sending.value = true
  try {
    await requestPasswordReset(email.value.trim(), locale.value)
    ElMessage.success(t('passwordResetSent'))
  } catch (err) {
    error.value = readForgotPasswordError(err)
  } finally {
    sending.value = false
  }
}

function readForgotPasswordError(err: unknown) {
  if (err instanceof ApiError && err.status === 400) {
    return t('invalidEmail')
  }
  return err instanceof Error ? err.message : String(err)
}
</script>

<template>
  <main class="login-shell">
    <el-button class="login-language home-language-button" :aria-label="t('language')" @click="toggleLocale">
      {{ nextLocaleLabel }}
    </el-button>
    <section class="login-stage">
      <el-form class="login-panel" @submit.prevent="sendPasswordReset">
        <div class="login-panel-heading">
          <h1>{{ t('forgotPasswordTitle') }}</h1>
          <p>{{ t('forgotPasswordHint') }}</p>
        </div>
        <div class="login-fields">
          <label class="login-field">
            <span>{{ t('email') }}</span>
            <el-input v-model="email" :prefix-icon="User" :placeholder="t('loginAccountPlaceholder')" type="email" size="large" />
          </label>
        </div>
        <el-button class="login-submit" native-type="submit" type="primary" size="large" :loading="sending">
          {{ t('sendPasswordReset') }}
        </el-button>
        <RouterLink class="login-secondary-link" to="/login">{{ t('backToLogin') }}</RouterLink>
        <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" />
      </el-form>
    </section>
  </main>
</template>
