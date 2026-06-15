<script setup lang="ts">
import { ElMessage } from 'element-plus/es/components/message/index'
import type { InputInstance } from 'element-plus/es/components/input/index'
import { DocumentCopy, Key } from '@element-plus/icons-vue'
import { computed, nextTick, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { getSetupStatus, type ServicePolicy } from '../../api/policy'
import { createUserKey, createUserKeyDraft } from '../../api/userKeys'
import PublicHeader from '../../components/PublicHeader.vue'
import { useAsyncData } from '../../composables/useAsyncData'
import { useInstallScript } from '../../composables/useInstallScript'
import { useLocale } from '../../composables/useLocale'
import { ApiError, isSmtpConfigError, readError } from '../../utils/errors'

const { locale, t } = useLocale()
const { installScript, copyInstallScript } = useInstallScript(t)
const { data: servicePolicy } = useAsyncData<ServicePolicy | null>(() => getSetupStatus(), null)
const apiKeyDialogOpen = ref(false)
const emailInput = ref<InputInstance>()
const homeEmail = ref('')
const homeDraftId = ref('')
const homeMaskedDraftKey = ref('')
const homeKeyLoading = ref(false)
const homeKeySubmitting = ref(false)
const homeKeySent = ref(false)
const publicKeyClaimEnabled = computed(() => {
  const policy = servicePolicy.value
  return policy?.service_mode === 'paid' && policy.registration_enabled === true
})
let draftRequestId = 0

function openApiKeyDialog() {
  if (!publicKeyClaimEnabled.value) return
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
    if (err instanceof ApiError && err.message.includes('account pending approval')) {
      ElMessage.warning(t('accountPendingApproval'))
      apiKeyDialogOpen.value = false
      return
    }
    if (isSmtpConfigError(err)) {
      ElMessage.error(t('smtpEmailUnavailable'))
      return
    }
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
    <PublicHeader />

    <section class="home-view">
      <div class="home-intro">
        <h1>{{ t('tagline') }}</h1>
      </div>

      <div v-if="publicKeyClaimEnabled" class="home-actions">
        <el-button :icon="Key" size="large" type="primary" @click="openApiKeyDialog">
          {{ t('createApiKey') }}
        </el-button>
        <p>{{ t('getApiKeyByEmail') }}</p>
      </div>

      <el-dialog
        v-if="publicKeyClaimEnabled"
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
        <RouterLink class="install-help-link" to="/docs">{{ t('viewHelpDocs') }}</RouterLink>
      </section>
    </section>
  </div>
</template>
