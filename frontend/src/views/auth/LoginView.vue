<script setup lang="ts">
import { Lock, User } from '@element-plus/icons-vue'
import { ElMessage, type InputInstance } from 'element-plus'
import { nextTick, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { login as loginAccount, requestLoginVerificationCode } from '../../api/auth'
import type { LoginRole } from '../../api/auth'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { useAuthStore } from '../../stores/auth'
import { ApiError, readError } from '../../utils/errors'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const { locale, t } = useLocale()
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

  signingIn.value = true

  try {
    const data = await loginAccount(username.value, password.value, verificationCode.value)
    auth.setToken(data.token, data.role)
    await router.replace(readRedirectPath(data.role))
  } catch (err) {
    if (isVerificationRequiredError(err)) {
      showVerificationCode.value = true
      await nextTick()
      verificationInput.value?.focus()
    }
    error.value = readLoginError(err)
  } finally {
    signingIn.value = false
  }
}

async function sendVerificationCode() {
  error.value = ''
  if (!username.value.trim()) {
    error.value = t('emailRequired')
    return
  }

  sendingVerificationCode.value = true
  try {
    await requestLoginVerificationCode(username.value, locale.value)
    showVerificationCode.value = true
    ElMessage.success(t('loginVerificationCodeSent'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    sendingVerificationCode.value = false
  }
}

function isVerificationRequiredError(err: unknown) {
  return err instanceof ApiError && err.message.includes('verification code required')
}

function readLoginError(err: unknown) {
  if (isVerificationRequiredError(err)) {
    return t('loginVerificationRequired')
  }
  if (err instanceof ApiError && err.message.includes('invalid verification code')) {
    return t('loginVerificationInvalid')
  }
  if (err instanceof ApiError && err.message.includes('password must be at least 8 characters')) {
    return t('passwordMinLength')
  }
  if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
    return t('loginFailed')
  }
  return err instanceof Error ? err.message : String(err)
}

function readRedirectPath(role: LoginRole) {
  const redirect = route.query.redirect
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
          <h1>{{ t('loginTitle') }}</h1>
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
