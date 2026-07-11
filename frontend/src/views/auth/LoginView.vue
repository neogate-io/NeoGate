<script setup lang="ts">
import { Lock, User } from '@element-plus/icons-vue'
import { ElMessage, type InputInstance } from 'element-plus'
import { computed, nextTick, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { login as loginAccount, requestLoginVerificationCode } from '../../api/auth'
import type { LoginRole } from '../../api/auth'
import LocaleToggleButton from '../../components/common/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { useSiteBrand } from '../../composables/useSiteBrand'
import { useAuthStore } from '../../stores/auth'
import { ApiError, isSmtpConfigError, readError } from '../../utils/errors'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const { locale, t } = useLocale()
const { siteName } = useSiteBrand()
const loginTitle = computed(() => t('loginTitle', { siteName: siteName.value }))
const username = ref('')
const password = ref('')
const verificationCode = ref('')
const error = ref('')
const signingIn = ref(false)
const sendingVerificationCode = ref(false)
const showVerificationCode = ref(false)
const passwordInput = ref<InputInstance>()
const verificationInput = ref<InputInstance>()
const minPasswordLength = 8

watch(username, () => {
  showVerificationCode.value = false
  verificationCode.value = ''
})

function focusPasswordOnTab(event: KeyboardEvent) {
  if (event.shiftKey) return
  event.preventDefault()
  passwordInput.value?.focus()
}

async function login() {
  error.value = ''
  if (password.value.length < minPasswordLength) {
    error.value = t('passwordMinLength')
    return
  }

  await withLoading(signingIn, async () => {
    try {
      const data = await loginAccount(username.value, password.value, verificationCode.value)
      auth.setToken(data.token, data.role, data.requires_password_change === true)
      await router.replace(readRedirectPath(data.role))
    } catch (err) {
      if (isVerificationRequiredError(err)) {
        showVerificationCode.value = true
        await nextTick()
        verificationInput.value?.focus()
      }
      error.value = readLoginError(err)
    }
  })
}

async function sendVerificationCode() {
  error.value = ''
  if (!username.value.trim()) {
    error.value = t('emailRequired')
    return
  }

  await withLoading(sendingVerificationCode, async () => {
    try {
      await requestLoginVerificationCode(username.value, locale.value)
      showVerificationCode.value = true
      ElMessage.success(t('loginVerificationCodeSent'))
    } catch (err) {
      ElMessage.error(readLoginVerificationCodeError(err))
    }
  })
}

function isVerificationRequiredError(err: unknown) {
  return err instanceof ApiError && err.code === 'verification_code_required'
}

function readLoginError(err: unknown) {
  if (isVerificationRequiredError(err)) {
    return t('loginVerificationRequired')
  }
  if (err instanceof ApiError && err.code === 'invalid_verification_code') {
    return t('loginVerificationInvalid')
  }
  if (err instanceof ApiError && err.code === 'registration_closed') {
    return t('registrationClosed')
  }
  if (err instanceof ApiError && err.code === 'account_pending_approval') {
    return t('accountPendingApproval')
  }
  if (isSmtpConfigError(err)) {
    return t('smtpEmailUnavailable')
  }
  if (isLoginVerificationRateLimitedError(err)) {
    return t('loginVerificationRateLimited')
  }
  if (err instanceof ApiError && err.code === 'password_min_length') {
    return t('passwordMinLength')
  }
  if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
    return t('loginFailed')
  }
  return err instanceof Error ? err.message : String(err)
}

function readLoginVerificationCodeError(err: unknown) {
  if (isSmtpConfigError(err)) {
    return t('smtpEmailUnavailable')
  }
  if (isLoginVerificationRateLimitedError(err)) {
    return t('loginVerificationRateLimited')
  }
  return readError(err)
}

function isLoginVerificationRateLimitedError(err: unknown) {
  return err instanceof ApiError && err.code === 'login_verification_rate_limited'
}

function readRedirectPath(role: LoginRole) {
  const redirect = route.query.redirect
  if (role === 'user' && auth.requiresPasswordChange) {
    const target = typeof redirect === 'string' && redirect.startsWith('/home') ? redirect : ''
    return target ? `/change-password?redirect=${encodeURIComponent(target)}` : '/change-password'
  }
  if (role === 'admin') {
    return typeof redirect === 'string' && redirect.startsWith('/admin') ? redirect : '/admin'
  }
  return typeof redirect === 'string' && redirect.startsWith('/home') ? redirect : '/home'
}
</script>

<template>
  <main class="login-shell">
    <LocaleToggleButton class="login-language home-language-button" />
    <section class="login-stage">
      <el-form class="login-panel" @submit.prevent="login">
        <div class="login-panel-heading">
          <h1>{{ loginTitle }}</h1>
          <p>{{ t('loginEmailHint') }}</p>
        </div>
        <div class="login-fields">
          <label class="login-field">
            <span>{{ t('email') }}</span>
            <el-input
              v-model="username"
              :prefix-icon="User"
              :placeholder="t('loginAccountPlaceholder')"
              size="large"
              @keydown.tab="focusPasswordOnTab"
            />
          </label>
          <label class="login-field">
            <span class="login-field-row">
              <span>{{ t('password') }}</span>
              <RouterLink class="login-text-button" to="/forgot-password">{{
                t('forgotPassword')
              }}</RouterLink>
            </span>
            <el-input
              ref="passwordInput"
              v-model="password"
              :prefix-icon="Lock"
              :placeholder="t('password')"
              show-password
              size="large"
            />
          </label>
          <label v-if="showVerificationCode" class="login-field">
            <span class="login-field-row">
              <span>{{ t('verificationCode') }}</span>
              <span class="login-field-hint">{{ t('loginVerificationHint') }}</span>
            </span>
            <div class="login-code-row">
              <el-input
                ref="verificationInput"
                v-model="verificationCode"
                :placeholder="t('verificationCode')"
                maxlength="6"
                inputmode="numeric"
                size="large"
              />
              <el-button
                class="login-code-button"
                native-type="button"
                type="primary"
                :loading="sendingVerificationCode"
                @click="sendVerificationCode"
              >
                {{ t('sendVerificationCode') }}
              </el-button>
            </div>
          </label>
        </div>
        <el-button
          class="login-submit"
          native-type="submit"
          type="primary"
          size="large"
          :loading="signingIn"
        >
          {{ t('signIn') }}
        </el-button>
        <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" />
      </el-form>
    </section>
  </main>
</template>
