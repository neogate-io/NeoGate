<script setup lang="ts">
import { ElMessage } from 'element-plus/es/components/message/index'
import type { InputInstance } from 'element-plus/es/components/input/index'
import { DocumentCopy, Key, UserFilled } from '@element-plus/icons-vue'
import { computed, nextTick, ref } from 'vue'
import { createUserKey, createUserKeyDraft } from '../../api/userKeys'
import { useInstallScript } from '../../composables/useInstallScript'
import { useLocale } from '../../composables/useLocale'
import { useAuthStore } from '../../stores/auth'
import { readError } from '../../utils/errors'

const githubUrl = 'https://github.com/asf26/main'
const auth = useAuthStore()
const { locale, t, toggleLocale } = useLocale()
const { installScript, copyInstallScript } = useInstallScript(t)
const apiKeyDialogOpen = ref(false)
const emailInput = ref<InputInstance>()
const nextLocaleLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中'))
const homeEmail = ref('')
const homeDraftId = ref('')
const homeMaskedDraftKey = ref('')
const homeKeyLoading = ref(false)
const homeKeySubmitting = ref(false)
const homeKeySent = ref(false)
const dashboardLink = computed(() => (auth.isAdmin ? '/admin' : '/home/overview'))
let draftRequestId = 0

function openApiKeyDialog() {
  resetHomeApiKey()
  apiKeyDialogOpen.value = true
  void prepareHomeApiKey()
}

async function focusEmailInput() {
  await nextTick()
  emailInput.value?.focus()
}

async function createHomeApiKey() {
  if (!homeEmail.value.trim()) {
    ElMessage.warning(t('emailRequired'))
    return
  }

  if (!homeDraftId.value) {
    await prepareHomeApiKey()
    if (!homeDraftId.value) return
  }

  homeKeySubmitting.value = true
  try {
    await createUserKey(homeEmail.value.trim(), homeDraftId.value, locale.value)
    homeDraftId.value = ''
    homeMaskedDraftKey.value = ''
    homeKeySent.value = true
    ElMessage.success(t('apiKeySentToast'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    homeKeySubmitting.value = false
  }
}

async function prepareHomeApiKey() {
  const requestId = ++draftRequestId
  homeKeyLoading.value = true
  homeDraftId.value = ''
  homeMaskedDraftKey.value = ''
  try {
    const data = await createUserKeyDraft()
    if (requestId !== draftRequestId) return
    homeDraftId.value = data.draft_id
    homeMaskedDraftKey.value = data.masked_api_key
  } catch (err) {
    if (requestId !== draftRequestId) return
    ElMessage.error(readError(err))
  } finally {
    if (requestId === draftRequestId) {
      homeKeyLoading.value = false
    }
  }
}

function resetHomeApiKey() {
  draftRequestId += 1
  homeEmail.value = ''
  homeDraftId.value = ''
  homeMaskedDraftKey.value = ''
  homeKeyLoading.value = false
  homeKeySubmitting.value = false
  homeKeySent.value = false
}

</script>

<template>
  <div class="home-page">
    <header class="home-header">
      <RouterLink class="home-brand" to="/" :aria-label="t('appName')">
        <img class="home-brand-logo" src="/logo.svg" :alt="t('appName')" />
      </RouterLink>
      <nav class="home-nav" :aria-label="t('appName')">
        <RouterLink to="/">{{ t('home') }}</RouterLink>
        <RouterLink to="/docs">{{ t('docs') }}</RouterLink>
      </nav>
      <div class="home-header-actions">
        <el-button class="home-language-button" :aria-label="t('language')" @click="toggleLocale">
          {{ nextLocaleLabel }}
        </el-button>
        <a class="github-link" :href="githubUrl" target="_blank" rel="noreferrer" :aria-label="t('github')">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M12 2C6.48 2 2 6.58 2 12.26c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49 0-.24-.01-.88-.01-1.73-2.78.62-3.37-1.38-3.37-1.38-.45-1.19-1.11-1.5-1.11-1.5-.91-.64.07-.63.07-.63 1 .07 1.53 1.06 1.53 1.06.9 1.57 2.35 1.12 2.93.86.09-.67.35-1.12.63-1.38-2.22-.26-4.56-1.14-4.56-5.07 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.33 9.33 0 0 1 12 6.98c.85 0 1.7.12 2.5.34 1.9-1.33 2.74-1.05 2.74-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.94-2.34 4.81-4.57 5.07.36.32.68.95.68 1.92 0 1.38-.01 2.49-.01 2.83 0 .27.18.59.69.49A10.15 10.15 0 0 0 22 12.26C22 6.58 17.52 2 12 2Z"
            />
          </svg>
        </a>
        <RouterLink v-if="auth.isAuthed" class="home-login-link home-account-link" :to="dashboardLink" :aria-label="t('admin')">
          <el-icon><UserFilled /></el-icon>
        </RouterLink>
        <RouterLink v-else class="home-login-link" to="/login">{{ t('signIn') }}</RouterLink>
      </div>
    </header>

    <section class="home-view">
      <div class="home-intro">
        <h1>{{ t('tagline') }}</h1>
      </div>

      <div class="home-actions">
        <el-button :icon="Key" size="large" type="primary" @click="openApiKeyDialog">
          {{ t('createApiKey') }}
        </el-button>
        <p>{{ t('getApiKeyByEmail') }}</p>
      </div>

      <el-dialog
        v-model="apiKeyDialogOpen"
        class="api-key-dialog"
        :title="t('apiKeyDialogTitle')"
        width="min(560px, calc(100vw - 32px))"
        @opened="focusEmailInput"
        @closed="resetHomeApiKey"
      >
        <el-alert
          v-if="homeKeySent"
          :title="t('apiKeySentTitle')"
          type="success"
          show-icon
          :closable="false"
        />
        <p v-if="!homeKeySent" class="api-key-dialog-intro">
          {{ t('apiKeyEmailHint') }}
        </p>
        <div v-if="!homeKeySent" class="api-key-preview" aria-live="polite">
          <div>
            <span>{{ t('apiKeyPreviewLabel') }}</span>
            <code>{{ homeMaskedDraftKey || t('apiKeyPreparing') }}</code>
          </div>
        </div>
        <p v-else class="api-key-dialog-copy">
          {{ t('apiKeyEmailSentHint') }}
        </p>
        <el-form class="api-key-dialog-form" @submit.prevent="createHomeApiKey">
          <el-input
            ref="emailInput"
            v-model="homeEmail"
            class="api-key-email-input"
            :placeholder="t('email')"
            type="email"
            size="large"
            :disabled="homeKeySent || homeKeySubmitting"
          />
          <el-button
            v-if="!homeKeySent"
            class="api-key-send-button"
            :disabled="homeKeyLoading"
            :loading="homeKeySubmitting"
            native-type="submit"
            type="primary"
          >
            {{ t('sendApiKeyToEmail') }}
          </el-button>
        </el-form>
      </el-dialog>

      <section id="docs" class="install-panel">
        <h2>{{ t('installScript') }}</h2>
        <div class="install-command">
          <code>{{ installScript }}</code>
          <el-button :icon="DocumentCopy" type="primary" @click="copyInstallScript">
            {{ t('copy') }}
          </el-button>
        </div>
      </section>
    </section>
  </div>
</template>
