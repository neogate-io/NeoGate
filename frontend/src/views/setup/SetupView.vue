<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  ArrowLeft,
  ArrowRight,
  Briefcase,
  Check,
  CreditCard,
  Finished,
  Key,
  Refresh,
  Select,
  Setting,
  Tickets,
  Warning
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  bootstrapSetup,
  completeSetupWizard,
  fetchSetupUpstreamModels,
  getClusterEnvTemplate,
  getSetupProviders,
  getSetupStatus,
  testSetupDatabase,
  type ServiceMode,
  type ServicePolicy
} from '../../api/policy'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useLocale } from '../../composables/useLocale'
import type { ProviderRecord } from '../../types/admin'
import { usdToMicroUsd } from '../../utils/format'
import { readError } from '../../utils/errors'
import { splitCommaList } from '../../utils/channel'

type Protocol = 'openai' | 'anthropic'
type DatabaseInputMode = 'fields' | 'url'
type BusinessSetupStep = 'admin-password' | 'service-mode' | 'upstream' | 'smtp'

const businessSetupSteps: BusinessSetupStep[] = [
  'admin-password',
  'service-mode',
  'upstream',
  'smtp'
]

const router = useRouter()
const { t } = useLocale()
const loading = ref(false)
const saving = ref(false)
const fetchingModels = ref(false)
const generatingTemplate = ref(false)
const testingDatabase = ref(false)
const waitingForRestart = ref(false)
const restartWaitTimedOut = ref(false)
const status = ref<ServicePolicy | null>(null)
const providers = ref<ProviderRecord[]>([])
const envFile = ref('')
const clusterEnvTemplate = ref('')
const currentBusinessStep = ref<BusinessSetupStep>('admin-password')
const includeUpstream = ref(true)
const reviewingRuntimeConfig = ref(false)
const runtimeDatabaseChangeEnabled = ref(false)

const bootstrapForm = reactive({
  databaseInputMode: 'fields' as DatabaseInputMode,
  databaseUrl: 'postgres://localhost/neogate',
  databaseHost: 'localhost',
  databasePort: 5432,
  databaseName: 'neogate',
  databaseUser: '',
  databasePassword: '',
  databaseSslMode: 'auto',
  siteName: 'NeoGate',
  publicBaseUrl: 'http://127.0.0.1:8080'
})

const setupForm = reactive({
  adminPassword: '',
  confirmPassword: '',
  serviceMode: 'internal' as ServiceMode,
  creditRequired: false,
  provider: 'openai',
  protocol: 'openai' as Protocol,
  channelName: '',
  baseUrl: '',
  secret: '',
  models: ''
})

const smtpForm = reactive({
  enabled: false,
  host: '',
  port: 587,
  tls: true,
  username: '',
  password: '',
  fromEmail: '',
  fromName: '',
  subjectPrefix: ''
})

const paymentForm = reactive({
  enabled: false,
  apiUrl: 'https://zpayz.cn/submit.php',
  merchantId: '',
  secretKey: '',
  payType: 'wxpay',
  siteName: 'NeoGate'
})

const prices = ref<Array<{
  model: string
  inputUsd: number
  outputUsd: number
  enabled: boolean
}>>([])

const modeOptions = computed(() => [
  {
    value: 'internal' as const,
    title: t('setupInternalMode'),
    description: t('setupInternalModeDescription'),
    icon: Briefcase
  },
  {
    value: 'paid' as const,
    title: t('setupPaidMode'),
    description: t('setupPaidModeDescription'),
    icon: CreditCard
  }
])

const setupSteps = computed(() => [
  {
    key: 'runtime',
    title: t('setupStepRuntime'),
    description: t('setupStepRuntimeDescription'),
    done: !status.value?.bootstrap_required && !reviewingRuntimeConfig.value,
    active: Boolean(status.value?.bootstrap_required) || reviewingRuntimeConfig.value
  },
  {
    key: 'admin-password',
    title: t('setupStepAdminPassword'),
    description: t('setupStepAdminPasswordDescription'),
    done: isBusinessStepDone('admin-password'),
    active: isBusinessStepActive('admin-password')
  },
  {
    key: 'service-mode',
    title: t('setupStepServiceMode'),
    description: t('setupStepServiceModeDescription'),
    done: isBusinessStepDone('service-mode'),
    active: isBusinessStepActive('service-mode')
  },
  {
    key: 'upstream',
    title: t('setupStepUpstream'),
    description: t('setupStepUpstreamDescription'),
    done: isBusinessStepDone('upstream'),
    active: isBusinessStepActive('upstream')
  },
  {
    key: 'smtp',
    title: t('setupStepSmtp'),
    description: t('setupStepSmtpDescription'),
    done: isBusinessStepDone('smtp'),
    active: isBusinessStepActive('smtp')
  }
])

const databaseInputModeOptions = computed(() => [
  { label: t('databaseModeFields'), value: 'fields' },
  { label: t('databaseModeUrl'), value: 'url' }
])

const databaseSslModeOptions = computed(() => [
  { label: t('databaseSslAuto'), value: 'auto' },
  { label: t('databaseSslDisable'), value: 'disable' },
  { label: t('databaseSslRequire'), value: 'require' }
])

const generatedDatabaseUrlPreview = computed(() => buildDatabaseUrl(true))

const selectedProvider = computed(() =>
  providers.value.find((provider) => provider.code === setupForm.provider)
)
const bootstrapMissingDatabase = computed(() => !status.value?.database_configured)
const clusterBlocked = computed(
  () => status.value?.bootstrap_required && status.value.runtime_mode === 'distributed'
)
const canConfigureEnv = computed(
  () => status.value?.bootstrap_required && status.value.env_write_supported
)
const canReviewRuntimeConfig = computed(
  () =>
    Boolean(status.value) &&
    !status.value?.bootstrap_required &&
    !status.value?.setup_completed &&
    status.value?.env_write_supported
)
const showRuntimeConfiguration = computed(
  () => canConfigureEnv.value || reviewingRuntimeConfig.value
)
const showBusinessSetup = computed(
  () =>
    status.value &&
    !status.value.bootstrap_required &&
    !status.value.setup_completed &&
    !reviewingRuntimeConfig.value
)

watch(
  () => [setupForm.provider, setupForm.protocol],
  () => applyProviderDefaults()
)

watch(
  () => setupForm.models,
  () => syncPriceRows()
)

async function load() {
  loading.value = true
  try {
    status.value = await getSetupStatus()
    if (status.value.setup_completed) {
      await router.replace('/login')
      return
    }
    bootstrapForm.siteName = status.value.site_name || bootstrapForm.siteName
    bootstrapForm.publicBaseUrl = status.value.public_base_url || bootstrapForm.publicBaseUrl
    paymentForm.siteName = status.value.site_name || paymentForm.siteName

    if (!status.value.bootstrap_required) {
      providers.value = await getSetupProviders()
      if (providers.value.length > 0 && !selectedProvider.value) {
        setupForm.provider = providers.value[0].code
      }
      applyProviderDefaults()
    }
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function saveBootstrap() {
  saving.value = true
  try {
    const result = await bootstrapSetup({
      database_url:
        bootstrapMissingDatabase.value || runtimeDatabaseChangeEnabled.value
          ? buildDatabaseUrl(false)
          : null,
      site_name: bootstrapForm.siteName,
      public_base_url: bootstrapForm.publicBaseUrl
    })
    envFile.value = result.env_file
    waitingForRestart.value = true
    restartWaitTimedOut.value = false
    ElMessage.success(t('runtimeConfigSaved'))
    if (reviewingRuntimeConfig.value) {
      waitingForRestart.value = false
      return
    }
    await waitForRuntimeRestart()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
  }
}

async function handleRuntimeSubmit() {
  await saveBootstrap()
}

async function waitForRuntimeRestart() {
  waitingForRestart.value = true
  restartWaitTimedOut.value = false
  const deadline = Date.now() + 120_000
  while (Date.now() < deadline) {
    await sleep(1500)
    try {
      const nextStatus = await getSetupStatus()
      if (nextStatus.setup_completed) {
        await router.replace('/login')
        return
      }
      if (!nextStatus.bootstrap_required) {
        status.value = nextStatus
        envFile.value = ''
        waitingForRestart.value = false
        restartWaitTimedOut.value = false
        providers.value = await getSetupProviders()
        if (providers.value.length > 0 && !selectedProvider.value) {
          setupForm.provider = providers.value[0].code
        }
        applyProviderDefaults()
        return
      }
    } catch {
      // The backend is expected to be temporarily unavailable while it restarts.
    }
  }
  waitingForRestart.value = false
  restartWaitTimedOut.value = true
}

async function testDatabaseConnection() {
  testingDatabase.value = true
  try {
    await testSetupDatabase({ database_url: buildDatabaseUrl(false) })
    ElMessage.success(t('databaseConnectionSucceeded'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    testingDatabase.value = false
  }
}

async function generateClusterTemplate() {
  generatingTemplate.value = true
  try {
    clusterEnvTemplate.value = (await getClusterEnvTemplate()).env_text
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    generatingTemplate.value = false
  }
}

async function fetchModels() {
  if (!setupForm.baseUrl.trim() || !setupForm.secret.trim()) {
    ElMessage.error(t('upstreamKeyRequired'))
    return
  }
  fetchingModels.value = true
  try {
    const result = await fetchSetupUpstreamModels({
      provider: setupForm.provider,
      protocol: setupForm.protocol,
      base_url: setupForm.baseUrl,
      secret: setupForm.secret
    })
    setupForm.models = result.models.join(', ')
    syncPriceRows()
    ElMessage.success(t('modelsFetched'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    fetchingModels.value = false
  }
}

async function submitSetup() {
  if (!validateSetup()) return
  saving.value = true
  try {
    const channel = includeUpstream.value
      ? {
          provider: setupForm.provider,
          name: setupForm.channelName.trim(),
          protocol: setupForm.protocol,
          base_url: setupForm.baseUrl.trim(),
          models: splitCommaList(setupForm.models),
          secret: setupForm.secret
        }
      : null
    await completeSetupWizard({
      admin_password: setupForm.adminPassword,
      service_mode: setupForm.serviceMode,
      credit_required: setupForm.serviceMode === 'internal' ? setupForm.creditRequired : true,
      channel,
      prices: includeUpstream.value
        ? prices.value.map((price) => ({
            provider: setupForm.provider,
            model: price.model,
            input_price_usd_micros: usdToMicroUsd(price.inputUsd),
            output_price_usd_micros: usdToMicroUsd(price.outputUsd),
            enabled: price.enabled
          }))
        : [],
      smtp: smtpForm.enabled ? {
        smtp_host: smtpForm.host,
        smtp_port: smtpForm.port,
        smtp_username: smtpForm.username || null,
        smtp_password: smtpForm.password || null,
        clear_smtp_password: false,
        smtp_tls: smtpForm.tls,
        from_email: smtpForm.fromEmail,
        from_name: smtpForm.fromName || null,
        subject_prefix: smtpForm.subjectPrefix || null
      } : null,
      payment: setupForm.serviceMode === 'paid' && paymentForm.enabled ? {
        payment_enabled: true,
        return_base_url: status.value?.public_base_url || bootstrapForm.publicBaseUrl,
        zpay_api_url: paymentForm.apiUrl,
        zpay_merchant_id: paymentForm.merchantId || null,
        zpay_secret_key: paymentForm.secretKey || null,
        clear_zpay_secret_key: false,
        zpay_default_pay_type: paymentForm.payType,
        zpay_site_name: paymentForm.siteName
      } : null
    })
    ElMessage.success(t('setupCompleted'))
    await router.replace('/login')
  } catch (err) {
    ElMessage.error(readError(err))
    await load()
  } finally {
    saving.value = false
  }
}

function validateSetup() {
  if (!validateAdminStep()) return false
  if (includeUpstream.value && !validateUpstreamStep()) return false
  if (!validateSmtpStep()) return false
  return true
}

function validateAdminStep() {
  if (!setupForm.adminPassword || setupForm.adminPassword.length < 8) {
    ElMessage.error(t('passwordMinLength'))
    return false
  }
  if (setupForm.adminPassword !== setupForm.confirmPassword) {
    ElMessage.error(t('adminPasswordMismatch'))
    return false
  }
  return true
}

function validateUpstreamStep() {
  if (!setupForm.provider.trim()) {
    ElMessage.error(t('providerRequired'))
    return false
  }
  if (!setupForm.channelName.trim()) {
    ElMessage.error(t('channelNameRequired'))
    return false
  }
  if (!setupForm.baseUrl.trim()) {
    ElMessage.error(t('baseUrlRequired'))
    return false
  }
  if (!setupForm.secret.trim()) {
    ElMessage.error(t('upstreamKeyRequired'))
    return false
  }
  if (splitCommaList(setupForm.models).length === 0) {
    ElMessage.error(t('modelsFetchRequired'))
    return false
  }
  if (prices.value.length === 0) {
    ElMessage.error(t('priceModelRequired'))
    return false
  }
  return true
}

function validateSmtpStep() {
  if (!smtpForm.enabled) return true
  if (!smtpForm.host.trim()) {
    ElMessage.error(t('smtpHostRequired'))
    return false
  }
  if (!smtpForm.fromEmail.trim()) {
    ElMessage.error(t('smtpFromEmailRequired'))
    return false
  }
  return true
}

function goToNextBusinessStep() {
  if (currentBusinessStep.value === 'admin-password' && !validateAdminStep()) return
  if (currentBusinessStep.value === 'upstream' && !validateUpstreamStep()) return
  if (currentBusinessStep.value === 'upstream') {
    includeUpstream.value = true
  }
  const nextIndex = businessSetupSteps.indexOf(currentBusinessStep.value) + 1
  currentBusinessStep.value = businessSetupSteps[nextIndex] ?? currentBusinessStep.value
}

function goToPreviousBusinessStep() {
  if (currentBusinessStep.value === 'admin-password' && canReviewRuntimeConfig.value) {
    reviewingRuntimeConfig.value = true
    runtimeDatabaseChangeEnabled.value = false
    bootstrapForm.databaseInputMode = 'url'
    bootstrapForm.databaseUrl = ''
    return
  }
  const previousIndex = businessSetupSteps.indexOf(currentBusinessStep.value) - 1
  currentBusinessStep.value = businessSetupSteps[previousIndex] ?? currentBusinessStep.value
}

function returnToBusinessSetup() {
  reviewingRuntimeConfig.value = false
  runtimeDatabaseChangeEnabled.value = false
}

function skipUpstreamStep() {
  includeUpstream.value = false
  currentBusinessStep.value = 'smtp'
}

async function skipSmtpAndSubmit() {
  smtpForm.enabled = false
  await submitSetup()
}

async function handleBusinessSubmit() {
  if (currentBusinessStep.value === 'smtp') {
    await submitSetup()
    return
  }
  goToNextBusinessStep()
}

function isBusinessStepActive(step: BusinessSetupStep) {
  return showBusinessSetup.value && currentBusinessStep.value === step
}

function isBusinessStepDone(step: BusinessSetupStep) {
  if (!showBusinessSetup.value) return false
  return businessSetupSteps.indexOf(currentBusinessStep.value) > businessSetupSteps.indexOf(step)
}

function applyProviderDefaults() {
  const provider = selectedProvider.value
  if (!provider) return
  setupForm.channelName = setupForm.channelName || provider.display_name
  const endpoint = provider.default_endpoints.find(
    (endpoint) => endpoint.protocol === setupForm.protocol
  )
  setupForm.baseUrl = endpoint?.base_url || setupForm.baseUrl
}

function syncPriceRows() {
  const existing = new Map(prices.value.map((price) => [price.model, price]))
  prices.value = splitCommaList(setupForm.models).map((model) => ({
    model,
    inputUsd: existing.get(model)?.inputUsd ?? 0,
    outputUsd: existing.get(model)?.outputUsd ?? 0,
    enabled: existing.get(model)?.enabled ?? true
  }))
}

function buildDatabaseUrl(maskPassword: boolean) {
  if (bootstrapForm.databaseInputMode === 'url') {
    return bootstrapForm.databaseUrl.trim()
  }

  const host = bootstrapForm.databaseHost.trim() || 'localhost'
  const databaseName = bootstrapForm.databaseName.trim() || 'neogate'
  const user = bootstrapForm.databaseUser.trim()
  const password = bootstrapForm.databasePassword
  const passwordValue = maskPassword && password ? '******' : password
  const auth = user
    ? `${encodeURIComponent(user)}${passwordValue ? `:${encodeURIComponent(passwordValue)}` : ''}@`
    : ''
  const normalizedHost = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host
  const port = Number(bootstrapForm.databasePort)
  const portPart = Number.isFinite(port) && port > 0 ? `:${port}` : ''
  const sslMode = bootstrapForm.databaseSslMode === 'auto' ? '' : bootstrapForm.databaseSslMode
  const query = sslMode ? `?sslmode=${encodeURIComponent(sslMode)}` : ''

  return `postgres://${auth}${normalizedHost}${portPart}/${encodeURIComponent(databaseName)}${query}`
}

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

onMounted(load)
</script>

<template>
  <main class="setup-shell">
    <LocaleToggleButton class="setup-language home-language-button" />
    <section v-loading="loading" class="setup-stage">
      <aside class="setup-brief">
        <div class="setup-logo-lockup">
          <img src="/logo.svg" alt="NeoGate" />
          <span>{{ t('setupBrandKicker') }}</span>
        </div>
        <div class="setup-heading">
          <h1>{{ t('setupTitle') }}</h1>
          <p>{{ t('setupSubtitle') }}</p>
        </div>
        <ol class="setup-steps">
          <li
            v-for="step in setupSteps"
            :key="step.key"
            :class="{ active: step.active, done: step.done }"
          >
            <span class="setup-step-mark">
              <el-icon><Check /></el-icon>
            </span>
            <span>
              <strong>{{ step.title }}</strong>
              <small>{{ step.description }}</small>
            </span>
          </li>
        </ol>
      </aside>

      <section v-if="status?.restart_required || envFile" class="setup-panel">
        <div class="setup-panel-title">
          <span class="setup-title-icon warning">
            <el-icon><Warning /></el-icon>
          </span>
          <div>
            <h2>{{ waitingForRestart ? t('runtimeRestarting') : t('restartRequired') }}</h2>
            <p>
              {{
                waitingForRestart
                  ? t('runtimeRestartingDescription')
                  : t('restartRequiredDescription')
              }}
            </p>
          </div>
        </div>
        <div class="setup-env-file">
          <span>{{ t('configWrittenTo') }}</span>
          <code>{{ envFile || '.env' }}</code>
        </div>
        <p v-if="restartWaitTimedOut" class="setup-warning-text">
          {{ t('runtimeRestartTimedOut') }}
        </p>
        <div v-if="restartWaitTimedOut" class="setup-field-actions">
          <el-button :loading="waitingForRestart" @click="waitForRuntimeRestart">
            {{ t('retryRuntimeCheck') }}
          </el-button>
        </div>
      </section>

      <section v-else-if="clusterBlocked" class="setup-panel">
        <div class="setup-panel-title">
          <span class="setup-title-icon warning">
            <el-icon><Warning /></el-icon>
          </span>
          <div>
            <h2>{{ t('clusterConfigRequired') }}</h2>
            <p>{{ t('clusterConfigDescription') }}</p>
          </div>
        </div>
        <ul class="setup-check-list">
          <li :class="{ ok: status?.database_configured }">DATABASE_URL</li>
          <li :class="{ ok: status?.redis_configured }">REDIS_URL</li>
          <li :class="{ ok: status?.secrets_configured }">ADMIN_TOKEN_SECRET / UPSTREAM_SECRET_KEY</li>
          <li :class="{ ok: status?.site_configured }">SITE_NAME / PUBLIC_BASE_URL</li>
        </ul>
        <el-button :loading="generatingTemplate" @click="generateClusterTemplate">
          {{ t('generateEnvTemplate') }}
        </el-button>
        <el-input
          v-if="clusterEnvTemplate"
          v-model="clusterEnvTemplate"
          class="setup-textarea"
          :rows="8"
          type="textarea"
          readonly
        />
      </section>

      <section v-else-if="showRuntimeConfiguration" class="setup-panel">
        <div class="setup-panel-title">
          <span class="setup-title-icon">
            <el-icon><Setting /></el-icon>
          </span>
          <div>
            <h2>{{ t('runtimeConfiguration') }}</h2>
            <p>{{ t('runtimeConfigurationDescription') }}</p>
          </div>
        </div>
        <el-form label-position="top" @submit.prevent="handleRuntimeSubmit">
          <div class="setup-section">
            <div class="setup-section-heading compact-heading">
              <div>
                <h2>{{ t('siteConfiguration') }}</h2>
                <p>{{ t('siteConfigurationDescription') }}</p>
              </div>
            </div>
            <div class="setup-grid two">
              <el-form-item :label="t('siteNameLabel')">
                <el-input v-model="bootstrapForm.siteName" />
              </el-form-item>
              <el-form-item :label="t('publicBaseUrlLabel')">
                <el-input v-model="bootstrapForm.publicBaseUrl" />
              </el-form-item>
            </div>
          </div>

          <div v-if="bootstrapMissingDatabase || reviewingRuntimeConfig" class="setup-section">
            <div class="setup-section-heading compact-heading">
              <div>
                <h2>{{ t('databaseConfiguration') }}</h2>
                <p>{{ t('databaseConfigurationDescription') }}</p>
              </div>
            </div>
            <div
              v-if="reviewingRuntimeConfig && !bootstrapMissingDatabase"
              class="setup-inline-control"
            >
              <span>
                <strong>{{ t('changeDatabaseConfiguration') }}</strong>
                <small>{{ t('changeDatabaseConfigurationHint') }}</small>
              </span>
              <el-switch v-model="runtimeDatabaseChangeEnabled" />
            </div>
            <el-form-item
              v-if="bootstrapMissingDatabase || runtimeDatabaseChangeEnabled"
              :label="t('databaseConnectionMode')"
            >
              <el-segmented
                v-model="bootstrapForm.databaseInputMode"
                :options="databaseInputModeOptions"
              />
            </el-form-item>

            <template
              v-if="
                (bootstrapMissingDatabase || runtimeDatabaseChangeEnabled) &&
                bootstrapForm.databaseInputMode === 'fields'
              "
            >
              <div class="setup-grid two">
                <el-form-item :label="t('databaseHostLabel')">
                  <el-input v-model="bootstrapForm.databaseHost" />
                </el-form-item>
                <el-form-item :label="t('databasePortLabel')">
                  <el-input-number
                    v-model="bootstrapForm.databasePort"
                    :min="1"
                    :max="65535"
                  />
                </el-form-item>
                <el-form-item :label="t('databaseNameLabel')">
                  <el-input v-model="bootstrapForm.databaseName" />
                </el-form-item>
                <el-form-item :label="t('databaseUserLabel')">
                  <el-input v-model="bootstrapForm.databaseUser" />
                </el-form-item>
                <el-form-item :label="t('databasePasswordLabel')">
                  <el-input v-model="bootstrapForm.databasePassword" show-password />
                </el-form-item>
                <el-form-item :label="t('databaseSslModeLabel')">
                  <el-select v-model="bootstrapForm.databaseSslMode">
                    <el-option
                      v-for="option in databaseSslModeOptions"
                      :key="option.value || 'auto'"
                      :label="option.label"
                      :value="option.value"
                    />
                  </el-select>
                </el-form-item>
              </div>
              <div class="setup-env-file">
                <span>{{ t('databaseGeneratedUrl') }}</span>
                <code>{{ generatedDatabaseUrlPreview }}</code>
              </div>
              <div class="setup-field-actions">
                <el-button :loading="testingDatabase" @click="testDatabaseConnection">
                  {{ t('testDatabaseConnection') }}
                </el-button>
              </div>
            </template>

            <el-form-item
              v-else-if="bootstrapMissingDatabase || runtimeDatabaseChangeEnabled"
              :label="t('databaseUrlLabel')"
            >
              <el-input v-model="bootstrapForm.databaseUrl" />
            </el-form-item>
            <div
              v-if="
                (bootstrapMissingDatabase || runtimeDatabaseChangeEnabled) &&
                bootstrapForm.databaseInputMode === 'url'
              "
              class="setup-field-actions"
            >
              <el-button :loading="testingDatabase" @click="testDatabaseConnection">
                {{ t('testDatabaseConnection') }}
              </el-button>
            </div>
          </div>

          <div class="setup-actions">
            <el-button
              v-if="reviewingRuntimeConfig"
              :icon="ArrowRight"
              @click="returnToBusinessSetup"
            >
              {{ t('nextStep') }}
            </el-button>
            <el-button type="primary" :loading="saving" native-type="submit">
              {{ t('saveRuntimeConfiguration') }}
            </el-button>
          </div>
        </el-form>
      </section>

      <section v-else-if="showBusinessSetup" class="setup-panel">
        <el-form label-position="top" @submit.prevent="handleBusinessSubmit">
          <div v-if="currentBusinessStep === 'admin-password'" class="setup-section">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><Key /></el-icon>
              </span>
              <div>
                <h2>{{ t('adminPasswordSettings') }}</h2>
                <p>{{ t('setupAdminPasswordHint') }}</p>
              </div>
            </div>
            <div class="setup-grid two">
              <el-form-item :label="t('newPassword')">
                <el-input v-model="setupForm.adminPassword" show-password type="password" />
              </el-form-item>
              <el-form-item :label="t('confirmNewPassword')">
                <el-input v-model="setupForm.confirmPassword" show-password type="password" />
              </el-form-item>
            </div>
          </div>

          <div v-else-if="currentBusinessStep === 'service-mode'" class="setup-section">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><Briefcase /></el-icon>
              </span>
              <div>
                <h2>{{ t('serviceMode') }}</h2>
                <p>{{ t('setupServiceModeHint') }}</p>
              </div>
            </div>
            <div class="setup-mode-grid" role="radiogroup" :aria-label="t('serviceMode')">
              <button
                v-for="item in modeOptions"
                :key="item.value"
                class="setup-mode-card"
                :class="{ active: setupForm.serviceMode === item.value }"
                type="button"
                @click="setupForm.serviceMode = item.value"
              >
                <span class="setup-mode-icon">
                  <el-icon><component :is="item.icon" /></el-icon>
                </span>
                <span class="setup-mode-copy">
                  <strong>{{ item.title }}</strong>
                  <span>{{ item.description }}</span>
                </span>
                <el-icon v-if="setupForm.serviceMode === item.value" class="setup-mode-check"><Check /></el-icon>
              </button>
            </div>
            <div v-if="setupForm.serviceMode === 'internal'" class="setup-inline-control">
              <span>
                <strong>{{ t('creditRequired') }}</strong>
                <small>{{ t('creditRequiredInternalHint') }}</small>
              </span>
              <el-switch v-model="setupForm.creditRequired" />
            </div>
            <template v-if="setupForm.serviceMode === 'paid'">
              <div class="setup-section compact nested-setup-section">
                <div class="setup-inline-control">
                  <span>
                    <strong>{{ t('paymentSettings') }}</strong>
                    <small>{{ t('setupPaymentHint') }}</small>
                  </span>
                  <el-switch v-model="paymentForm.enabled" />
                </div>
                <div v-if="paymentForm.enabled" class="setup-grid two optional-grid">
                  <el-form-item :label="t('zpayApiUrl')"><el-input v-model="paymentForm.apiUrl" /></el-form-item>
                  <el-form-item :label="t('zpaySiteName')"><el-input v-model="paymentForm.siteName" /></el-form-item>
                  <el-form-item :label="t('zpayMerchantId')"><el-input v-model="paymentForm.merchantId" /></el-form-item>
                  <el-form-item :label="t('zpaySecretKey')"><el-input v-model="paymentForm.secretKey" show-password /></el-form-item>
                </div>
              </div>
            </template>
          </div>

          <div v-else-if="currentBusinessStep === 'upstream'" class="setup-section">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><Tickets /></el-icon>
              </span>
              <div>
                <h2>{{ t('upstreamChannels') }}</h2>
                <p>{{ t('setupUpstreamHint') }}</p>
              </div>
            </div>
            <div class="setup-grid two">
              <el-form-item :label="t('provider')">
                <el-select v-model="setupForm.provider" filterable>
                  <el-option
                    v-for="provider in providers"
                    :key="provider.code"
                    :label="provider.display_name"
                    :value="provider.code"
                  >
                    <span class="provider-option"><ProviderIcon :provider="provider.code" />{{ provider.display_name }}</span>
                  </el-option>
                </el-select>
              </el-form-item>
              <el-form-item :label="t('protocol')">
                <el-segmented v-model="setupForm.protocol" :options="['openai', 'anthropic']" />
              </el-form-item>
              <el-form-item :label="t('name')">
                <el-input v-model="setupForm.channelName" :placeholder="t('channelNamePlaceholder')" />
              </el-form-item>
              <el-form-item :label="t('baseUrl')">
                <el-input v-model="setupForm.baseUrl" :placeholder="t('baseUrlPlaceholder')" />
              </el-form-item>
            </div>
            <el-form-item :label="t('upstreamApiKey')">
              <el-input v-model="setupForm.secret" :rows="4" show-password type="textarea" />
            </el-form-item>
            <el-form-item :label="t('models')">
              <div class="models-row">
                <el-input v-model="setupForm.models" :placeholder="t('modelsCommaSeparated')" />
                <el-button :icon="Refresh" :loading="fetchingModels" @click="fetchModels">
                  {{ t('autoFetch') }}
                </el-button>
              </div>
            </el-form-item>

            <div class="nested-setup-section">
              <div class="setup-section-heading">
                <span class="setup-title-icon">
                  <el-icon><Finished /></el-icon>
                </span>
                <div>
                  <h2>{{ t('modelPrices') }}</h2>
                  <p>{{ t('setupPricesHint') }}</p>
                </div>
              </div>
              <div class="price-list">
                <div class="price-row price-header">
                  <span>{{ t('model') }}</span>
                  <span>{{ t('inputShort') }}</span>
                  <span>{{ t('outputShort') }}</span>
                  <span>{{ t('status') }}</span>
                </div>
                <div v-for="price in prices" :key="price.model" class="price-row">
                  <span class="price-model">{{ price.model }}</span>
                  <el-input-number v-model="price.inputUsd" :min="0" :precision="6" :step="0.1" />
                  <el-input-number v-model="price.outputUsd" :min="0" :precision="6" :step="0.1" />
                  <el-switch v-model="price.enabled" />
                </div>
                <el-empty v-if="prices.length === 0" :description="t('modelsFetchRequired')" />
              </div>
            </div>
          </div>

          <div v-else-if="currentBusinessStep === 'smtp'" class="setup-section compact">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><Setting /></el-icon>
              </span>
              <div>
                <h2>{{ t('smtpSettings') }}</h2>
                <p>{{ t('setupSmtpHint') }}</p>
              </div>
            </div>
            <div class="setup-inline-control smtp-enable-control">
              <span>
                <strong>{{ t('setupEnableSmtp') }}</strong>
                <small>{{ t('setupEnableSmtpDescription') }}</small>
              </span>
              <el-switch v-model="smtpForm.enabled" />
            </div>
            <div v-if="smtpForm.enabled" class="setup-grid two optional-grid">
              <el-form-item :label="t('smtpHost')"><el-input v-model="smtpForm.host" /></el-form-item>
              <el-form-item :label="t('smtpPort')"><el-input-number v-model="smtpForm.port" :min="1" :max="65535" /></el-form-item>
              <el-form-item :label="t('smtpUsername')"><el-input v-model="smtpForm.username" /></el-form-item>
              <el-form-item :label="t('smtpPassword')"><el-input v-model="smtpForm.password" show-password /></el-form-item>
              <el-form-item :label="t('mailFromEmail')"><el-input v-model="smtpForm.fromEmail" /></el-form-item>
              <el-form-item :label="t('mailFromName')"><el-input v-model="smtpForm.fromName" /></el-form-item>
            </div>
          </div>

          <div class="setup-actions sticky">
            <el-button
              v-if="currentBusinessStep !== 'admin-password' || canReviewRuntimeConfig"
              :icon="ArrowLeft"
              @click="goToPreviousBusinessStep"
            >
              {{ t('previousStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep === 'upstream'"
              @click="skipUpstreamStep"
            >
              {{ t('skipStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep !== 'smtp'"
              type="primary"
              :icon="ArrowRight"
              @click="goToNextBusinessStep"
            >
              {{ t('nextStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep === 'smtp'"
              :loading="saving && !smtpForm.enabled"
              :type="smtpForm.enabled ? undefined : 'primary'"
              @click="skipSmtpAndSubmit"
            >
              {{ t('skipStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep === 'smtp' && smtpForm.enabled"
              class="setup-submit"
              type="primary"
              :icon="Select"
              :loading="saving"
              @click="submitSetup"
            >
              {{ t('completeSetup') }}
            </el-button>
          </div>
        </el-form>
      </section>
    </section>
  </main>
</template>

<style scoped>
.setup-shell {
  background:
    linear-gradient(135deg, rgba(22, 139, 211, 0.1), rgba(5, 150, 105, 0.06) 42%, transparent 42%),
    #f7f8fa;
  min-height: 100vh;
  padding: 28px clamp(18px, 4vw, 44px);
  position: relative;
}

.setup-language {
  position: fixed;
  right: clamp(18px, 4vw, 44px);
  top: 22px;
  z-index: 2;
}

.setup-stage {
  align-items: start;
  display: grid;
  gap: clamp(22px, 4vw, 42px);
  grid-template-columns: minmax(260px, 360px) minmax(0, 760px);
  min-height: calc(100vh - 56px);
  margin: 0 auto;
  max-width: 1180px;
  padding-top: 34px;
}

.setup-brief {
  align-content: start;
  display: grid;
  gap: 26px;
  padding: 34px 0 0;
}

.setup-logo-lockup {
  align-items: center;
  display: flex;
  gap: 12px;
}

.setup-logo-lockup img {
  height: 42px;
  object-fit: contain;
  width: 144px;
}

.setup-logo-lockup span {
  border-left: 1px solid #d8e1ec;
  color: #64748b;
  font-size: 13px;
  font-weight: 700;
  line-height: 1;
  padding-left: 12px;
}

.setup-heading {
  display: grid;
  gap: 12px;
}

.setup-heading h1,
.setup-panel h2 {
  color: #111827;
  letter-spacing: 0;
  margin: 0;
}

.setup-heading h1 {
  font-size: clamp(34px, 5vw, 54px);
  font-weight: 840;
  line-height: 1.02;
}

.setup-heading p,
.setup-panel p {
  color: #64748b;
  font-size: 14px;
  line-height: 1.7;
  margin: 0;
}

.setup-steps {
  display: grid;
  gap: 12px;
  list-style: none;
  margin: 10px 0 0;
  padding: 0;
}

.setup-steps li {
  align-items: start;
  display: grid;
  gap: 12px;
  grid-template-columns: 28px minmax(0, 1fr);
}

.setup-step-mark {
  align-items: center;
  background: #e6edf5;
  border-radius: 50%;
  color: #7a8798;
  display: inline-flex;
  height: 28px;
  justify-content: center;
  width: 28px;
}

.setup-steps li.done .setup-step-mark {
  background: #dcfce7;
  color: #059669;
}

.setup-steps li.active .setup-step-mark {
  background: #eaf6fd;
  color: #168bd3;
}

.setup-steps li.active strong {
  color: #168bd3;
}

.setup-steps strong,
.setup-inline-control strong {
  color: #172033;
  display: block;
  font-size: 14px;
  font-weight: 800;
  line-height: 1.35;
}

.setup-steps small,
.setup-inline-control small {
  color: #738093;
  display: block;
  font-size: 12px;
  line-height: 1.55;
  margin-top: 3px;
}

.setup-panel {
  align-content: start;
  background: #ffffff;
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  box-shadow: 0 18px 48px rgba(15, 23, 42, 0.08);
  display: grid;
  gap: 22px;
  margin-bottom: 28px;
  padding: clamp(20px, 3vw, 30px);
}

.setup-panel > .el-form {
  display: grid;
  gap: 20px;
}

.setup-panel-title,
.setup-section-heading {
  align-items: start;
  display: grid;
  gap: 12px;
  grid-template-columns: 42px minmax(0, 1fr);
}

.setup-section-heading.compact-heading {
  grid-template-columns: minmax(0, 1fr);
}

.setup-title-icon {
  align-items: center;
  background: #eaf6fd;
  border: 1px solid #d3edf9;
  border-radius: 8px;
  color: #168bd3;
  display: inline-flex;
  height: 42px;
  justify-content: center;
  width: 42px;
}

.setup-title-icon.warning {
  background: #fff7ed;
  border-color: #fed7aa;
  color: #ea580c;
}

.setup-panel h2 {
  font-size: 18px;
  font-weight: 820;
  margin: 0;
}

.setup-section {
  border-top: 1px solid #edf1f5;
  display: grid;
  gap: 16px;
  padding-top: 22px;
}

.setup-section:first-child {
  border-top: 0;
  padding-top: 0;
}

.setup-section.compact {
  gap: 12px;
}

.nested-setup-section {
  border-top: 1px solid #edf1f5;
  display: grid;
  gap: 16px;
  padding-top: 18px;
}

.setup-grid {
  display: grid;
  gap: 14px;
}

.setup-grid.two {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.setup-grid .el-input-number,
.setup-grid .el-select {
  width: 100%;
}

.setup-mode-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.setup-mode-card {
  align-items: start;
  background: #ffffff;
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  color: #111827;
  cursor: pointer;
  display: grid;
  gap: 12px;
  min-height: 150px;
  padding: 16px;
  position: relative;
  text-align: left;
  transition:
    border-color 0.15s ease,
    box-shadow 0.15s ease,
    transform 0.15s ease;
}

.setup-mode-card:hover {
  border-color: #b7dcf2;
  transform: translateY(-1px);
}

.setup-mode-card.active {
  border-color: #168bd3;
  box-shadow: 0 16px 40px rgba(22, 139, 211, 0.16);
}

.setup-mode-icon {
  align-items: center;
  background: #eef6fb;
  border-radius: 8px;
  color: #168bd3;
  display: inline-flex;
  height: 40px;
  justify-content: center;
  width: 40px;
}

.setup-mode-copy {
  display: grid;
  gap: 8px;
}

.setup-mode-copy strong {
  font-size: 17px;
  font-weight: 820;
}

.setup-mode-copy span {
  color: #64748b;
  font-size: 14px;
  line-height: 1.6;
}

.setup-mode-check {
  color: #168bd3;
  position: absolute;
  right: 16px;
  top: 16px;
}

.setup-inline-control {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  display: flex;
  gap: 16px;
  justify-content: space-between;
  padding: 14px 16px;
}

.models-row {
  display: grid;
  gap: 10px;
  grid-template-columns: minmax(0, 1fr) auto;
  width: 100%;
}

.price-list {
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  display: grid;
  overflow: hidden;
}

.price-row {
  align-items: center;
  display: grid;
  gap: 10px;
  grid-template-columns: minmax(160px, 1fr) 170px 170px 70px;
  padding: 10px;
}

.price-header {
  background: #f8fafc;
  color: #64748b;
  font-size: 12px;
  font-weight: 800;
  padding-bottom: 8px;
  padding-top: 8px;
}

.price-row + .price-row {
  border-top: 1px solid #e2e8f0;
}

.price-model {
  color: #172033;
  font-size: 13px;
  font-weight: 700;
  overflow-wrap: anywhere;
}

.setup-check-list {
  color: #dc2626;
  display: grid;
  gap: 8px;
  margin: 0;
  padding-left: 18px;
}

.setup-check-list .ok {
  color: #059669;
}

.setup-textarea {
  margin-top: 8px;
}

.provider-option {
  align-items: center;
  display: flex;
  gap: 8px;
}

.optional-grid {
  margin-top: 12px;
}

.smtp-enable-control {
  margin-top: 2px;
}

.setup-actions {
  align-items: center;
  display: flex;
  gap: 10px;
  justify-content: flex-end;
  flex-wrap: wrap;
}

.setup-field-actions {
  align-items: center;
  display: flex;
  justify-content: flex-start;
  margin-top: -6px;
}

.setup-actions.sticky {
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.86), #ffffff);
  border-top: 1px solid #edf1f5;
  bottom: 0;
  margin: 2px -8px -10px;
  padding: 16px 8px 2px;
  position: sticky;
  z-index: 1;
}

.setup-submit {
  min-width: 148px;
}

.setup-env-file {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  display: flex;
  gap: 10px;
  justify-content: space-between;
  padding: 14px 16px;
}

.setup-env-file span {
  color: #64748b;
  font-size: 13px;
}

.setup-env-file code {
  color: #172033;
  font-size: 13px;
  overflow-wrap: anywhere;
}

.setup-warning-text {
  background: #fff7ed;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  color: #9a3412 !important;
  padding: 12px 14px;
}

@media (max-width: 980px) {
  .setup-stage {
    grid-template-columns: 1fr;
    min-height: auto;
    padding-top: 54px;
  }

  .setup-brief {
    gap: 18px;
    padding-top: 0;
  }

  .setup-steps {
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
  }

  .setup-steps li {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 760px) {
  .setup-shell {
    padding: 18px;
  }

  .setup-language {
    right: 18px;
    top: 18px;
  }

  .setup-grid.two,
  .setup-mode-grid,
  .price-row,
  .setup-steps {
    grid-template-columns: 1fr;
  }

  .price-header {
    display: none;
  }

  .price-row {
    align-items: start;
  }

  .models-row {
    grid-template-columns: 1fr;
  }

  .setup-inline-control,
  .setup-env-file {
    align-items: stretch;
    flex-direction: column;
  }

  .setup-actions {
    justify-content: stretch;
  }

  .setup-actions .el-button,
  .setup-field-actions .el-button {
    width: 100%;
  }
}
</style>
