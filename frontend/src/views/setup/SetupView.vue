<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  ArrowLeft,
  ArrowRight,
  Briefcase,
  Check,
  Connection,
  CreditCard,
  Key,
  Lock,
  Message,
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
  syncSetupPricingTemplates,
  testSetupDatabase,
  testSetupSmtpSetting,
  type ServiceMode,
  type ServicePolicy
} from '../../api/policy'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import ProviderIcon from '../../components/ProviderIcon.vue'
import { useLocale } from '../../composables/useLocale'
import type { ProviderRecord } from '../../types/admin'
import { microUsdToUsd, usdToMicroUsd } from '../../utils/format'
import { ApiError, readError } from '../../utils/errors'
import { splitCommaList } from '../../utils/channel'
import { findPricingTemplate } from '../../utils/pricing'

type Protocol = 'openai' | 'anthropic'
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
const configuringPrices = ref(false)
const testingSmtp = ref(false)
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
  databaseHost: 'localhost',
  databasePort: 5432,
  databaseName: 'neogate',
  databaseUser: '',
  databasePassword: '',
  databaseSslMode: 'auto',
  siteName: 'NeoGate',
  publicBaseUrl: defaultPublicBaseUrl()
})

const setupForm = reactive({
  adminUsername: 'admin',
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
  enabled: true,
  host: '',
  port: 587,
  tls: true,
  username: '',
  password: '',
  fromEmail: '',
  fromName: '',
  subjectPrefix: ''
})

const smtpPortInput = computed({
  get: () => (smtpForm.port > 0 ? String(smtpForm.port) : ''),
  set: (value: string) => {
    const digits = value.replace(/\D/g, '').slice(0, 5)
    if (!digits) {
      smtpForm.port = 0
      return
    }
    smtpForm.port = Math.min(Number(digits), 65535)
  }
})

const paymentForm = reactive({
  enabled: false,
  apiUrl: 'https://zpayz.cn/submit.php',
  merchantId: '',
  secretKey: '',
  payType: 'wxpay',
  siteName: 'NeoGate'
})

const prices = ref<
  Array<{
    model: string
    inputUsd: number
    outputUsd: number
    enabled: boolean
  }>
>([])

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

const databaseSslModeOptions = computed(() => [
  { label: t('databaseSslAuto'), value: 'auto' },
  { label: t('databaseSslDisable'), value: 'disable' },
  { label: t('databaseSslRequire'), value: 'require' }
])

const generatedDatabaseUrlPreview = computed(() => buildDatabaseUrl(true))
const completedStepCount = computed(() => setupSteps.value.filter((step) => step.done).length)
const setupProgressPercent = computed(() => {
  if (setupSteps.value.length === 0) return 0

  return Math.round((completedStepCount.value / setupSteps.value.length) * 100)
})

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
const passwordChecks = computed(() => [
  {
    key: 'length',
    label: t('passwordLengthCheck'),
    passed: setupForm.adminPassword.length >= 8
  },
  {
    key: 'match',
    label: t('passwordMatchCheck'),
    passed:
      setupForm.adminPassword.length > 0 &&
      setupForm.confirmPassword.length > 0 &&
      setupForm.adminPassword === setupForm.confirmPassword
  }
])
const passwordReady = computed(() => passwordChecks.value.every((item) => item.passed))

watch(
  () => setupForm.provider,
  () => applyProviderDefaults()
)

watch(
  () => setupForm.models,
  () => syncPriceRows()
)

watch(
  () => smtpForm.tls,
  (tls) => {
    smtpForm.port = tls ? 587 : 25
  }
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
  if (!validateRuntimeConfig()) return
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
  if (!validateDatabaseConfig()) return
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

async function syncAndApplyReferencePrices() {
  try {
    const { templates } = await syncSetupPricingTemplates()
    let applied = 0
    for (const price of prices.value) {
      const template = findPricingTemplate(templates, setupForm.provider, price.model)
      if (!template) continue
      price.inputUsd = microUsdToUsd(template.input_price_usd_micros)
      price.outputUsd = microUsdToUsd(template.output_price_usd_micros)
      applied += 1
    }
    if (applied > 0) {
      ElMessage.success(t('referencePricesApplied'))
    }
    return true
  } catch (err) {
    ElMessage.error(readReferenceSyncError(err))
    return false
  }
}

async function prepareUpstreamPrices() {
  syncPriceRows()
  if (prices.value.length === 0) {
    ElMessage.error(t('priceModelRequired'))
    return false
  }

  configuringPrices.value = true
  try {
    return await syncAndApplyReferencePrices()
  } finally {
    configuringPrices.value = false
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
      admin_username: setupForm.adminUsername.trim(),
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
      smtp: smtpForm.enabled ? smtpPayload() : null,
      payment:
        setupForm.serviceMode === 'paid' && paymentForm.enabled
          ? {
              payment_enabled: true,
              return_base_url: status.value?.public_base_url || bootstrapForm.publicBaseUrl,
              zpay_api_url: paymentForm.apiUrl,
              zpay_merchant_id: paymentForm.merchantId || null,
              zpay_secret_key: paymentForm.secretKey || null,
              clear_zpay_secret_key: false,
              zpay_default_pay_type: paymentForm.payType,
              zpay_site_name: paymentForm.siteName
            }
          : null
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

function validateRuntimeConfig() {
  if (!bootstrapForm.siteName.trim()) {
    ElMessage.error(t('siteNameRequired'))
    return false
  }
  if (!bootstrapForm.publicBaseUrl.trim()) {
    ElMessage.error(t('publicBaseUrlRequired'))
    return false
  }
  try {
    const publicUrl = new URL(bootstrapForm.publicBaseUrl.trim())
    if (!['http:', 'https:'].includes(publicUrl.protocol)) {
      throw new Error('Invalid protocol')
    }
  } catch {
    ElMessage.error(t('publicBaseUrlInvalid'))
    return false
  }
  return validateDatabaseConfig()
}

function validateDatabaseConfig() {
  if (!bootstrapMissingDatabase.value && !runtimeDatabaseChangeEnabled.value) return true
  const databaseUrl = buildDatabaseUrl(false).trim()
  if (!databaseUrl) {
    ElMessage.error(t('databaseUrlRequired'))
    return false
  }
  if (!databaseUrl.startsWith('postgres://') && !databaseUrl.startsWith('postgresql://')) {
    ElMessage.error(t('databaseUrlInvalid'))
    return false
  }
  return true
}

function validateAdminStep() {
  if (!setupForm.adminUsername.trim()) {
    ElMessage.error(t('adminUsernameRequired'))
    return false
  }
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
  return true
}

function validateSmtpStep() {
  if (!smtpForm.enabled) return true
  if (!smtpForm.host.trim()) {
    ElMessage.error(t('smtpHostRequired'))
    return false
  }
  if (!Number.isInteger(smtpForm.port) || smtpForm.port < 1 || smtpForm.port > 65535) {
    ElMessage.error(t('smtpPortInvalid'))
    return false
  }
  if (!smtpForm.fromEmail.trim()) {
    ElMessage.error(t('smtpFromEmailRequired'))
    return false
  }
  return true
}

function smtpPayload() {
  return {
    smtp_host: smtpForm.host,
    smtp_port: smtpForm.port,
    smtp_username: smtpForm.username || null,
    smtp_password: smtpForm.password || null,
    clear_smtp_password: false,
    smtp_tls: smtpForm.tls,
    from_email: smtpForm.fromEmail,
    from_name: smtpForm.fromName || null,
    subject_prefix: smtpForm.subjectPrefix || null
  }
}

async function sendSmtpTestEmail() {
  smtpForm.enabled = true
  if (!validateSmtpStep()) return
  testingSmtp.value = true
  try {
    await testSetupSmtpSetting(smtpPayload())
    ElMessage.success(t('smtpTestEmailSent'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    testingSmtp.value = false
  }
}

function readReferenceSyncError(err: unknown) {
  if (
    err instanceof ApiError &&
    err.status === 502 &&
    err.message.includes('pricing reference source')
  ) {
    return t('referencePricesSourceUnavailable')
  }

  return readError(err)
}

async function goToNextBusinessStep() {
  if (currentBusinessStep.value === 'admin-password' && !validateAdminStep()) return
  if (currentBusinessStep.value === 'upstream' && !validateUpstreamStep()) return
  if (currentBusinessStep.value === 'upstream') {
    if (!(await prepareUpstreamPrices())) return
    includeUpstream.value = true
  }
  const nextIndex = businessSetupSteps.indexOf(currentBusinessStep.value) + 1
  currentBusinessStep.value = businessSetupSteps[nextIndex] ?? currentBusinessStep.value
}

function goToPreviousBusinessStep() {
  if (currentBusinessStep.value === 'admin-password' && canReviewRuntimeConfig.value) {
    reviewingRuntimeConfig.value = true
    runtimeDatabaseChangeEnabled.value = false
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
  await goToNextBusinessStep()
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
  if (provider.code === 'custom') {
    setupForm.channelName = ''
    setupForm.baseUrl = ''
    setupForm.protocol = 'openai'
    return
  }

  setupForm.channelName = provider.display_name
  const endpoint =
    provider.default_endpoints.find((item) => item.protocol === 'openai' && item.base_url) ??
    provider.default_endpoints.find((item) => item.protocol === 'anthropic' && item.base_url)
  if (endpoint?.protocol === 'openai' || endpoint?.protocol === 'anthropic') {
    setupForm.protocol = endpoint.protocol
  }
  setupForm.baseUrl = endpoint?.base_url || ''
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

function defaultPublicBaseUrl() {
  if (typeof window === 'undefined') return 'http://127.0.0.1:8080'
  return window.location.origin
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
        <div class="setup-progress-card" :aria-label="t('setupProgress')">
          <div>
            <span>{{ t('setupProgress') }}</span>
            <strong>{{ completedStepCount }} / {{ setupSteps.length }}</strong>
          </div>
          <span
            class="setup-progress-track"
            role="progressbar"
            :aria-valuenow="setupProgressPercent"
            aria-valuemin="0"
            aria-valuemax="100"
          >
            <span class="setup-progress-fill" :style="{ width: `${setupProgressPercent}%` }" />
          </span>
        </div>
        <ol class="setup-steps">
          <li
            v-for="(step, index) in setupSteps"
            :key="step.key"
            :class="{ active: step.active, done: step.done }"
            :aria-current="step.active ? 'step' : undefined"
          >
            <span class="setup-step-mark">
              <el-icon><Check /></el-icon>
              <span class="setup-step-index">{{ index + 1 }}</span>
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
          <li :class="{ ok: status?.secrets_configured }">
            ADMIN_TOKEN_SECRET / UPSTREAM_SECRET_KEY
          </li>
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
            <template v-if="bootstrapMissingDatabase || runtimeDatabaseChangeEnabled">
              <div class="setup-grid two">
                <el-form-item :label="t('databaseHostLabel')">
                  <el-input v-model="bootstrapForm.databaseHost" />
                </el-form-item>
                <el-form-item :label="t('databasePortLabel')">
                  <el-input-number v-model="bootstrapForm.databasePort" :min="1" :max="65535" />
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
                <el-button
                  :disabled="saving"
                  :loading="testingDatabase"
                  @click="testDatabaseConnection"
                >
                  {{ t('testDatabaseConnection') }}
                </el-button>
              </div>
            </template>
          </div>

          <div class="setup-actions runtime-actions">
            <el-button
              v-if="reviewingRuntimeConfig"
              :icon="ArrowRight"
              :disabled="saving"
              @click="returnToBusinessSetup"
            >
              {{ t('nextStep') }}
            </el-button>
            <el-button
              type="primary"
              :disabled="testingDatabase || waitingForRestart"
              :loading="saving"
              native-type="submit"
            >
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
            <div class="setup-grid admin-credentials-grid">
              <el-form-item :label="t('username')">
                <el-input v-model="setupForm.adminUsername" />
              </el-form-item>
              <el-form-item :label="t('newPassword')">
                <el-input v-model="setupForm.adminPassword" show-password type="password" />
              </el-form-item>
              <el-form-item :label="t('confirmNewPassword')">
                <el-input v-model="setupForm.confirmPassword" show-password type="password" />
              </el-form-item>
            </div>
            <div class="setup-password-checks" :class="{ ready: passwordReady }">
              <span
                v-for="item in passwordChecks"
                :key="item.key"
                class="setup-check-pill"
                :class="{ ok: item.passed }"
              >
                <el-icon><Check /></el-icon>
                {{ item.label }}
              </span>
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
                role="radio"
                :aria-checked="setupForm.serviceMode === item.value"
                :disabled="saving"
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
                <el-icon v-if="setupForm.serviceMode === item.value" class="setup-mode-check"
                  ><Check
                /></el-icon>
              </button>
            </div>
            <div
              v-if="setupForm.serviceMode === 'internal'"
              class="setup-inline-control credit-required-control"
            >
              <span>
                <strong>{{ t('creditRequired') }}</strong>
                <small>{{ t('creditRequiredInternalHint') }}</small>
              </span>
              <el-switch v-model="setupForm.creditRequired" />
            </div>
            <template v-if="setupForm.serviceMode === 'paid'">
              <div class="setup-section compact nested-setup-section">
                <div class="setup-inline-control payment-enable-control">
                  <span>
                    <strong>{{ t('paymentSettings') }}</strong>
                    <small>{{ t('setupPaymentHint') }}</small>
                  </span>
                  <el-switch v-model="paymentForm.enabled" />
                </div>
                <div v-if="paymentForm.enabled" class="setup-grid two optional-grid">
                  <el-form-item :label="t('zpayApiUrl')"
                    ><el-input v-model="paymentForm.apiUrl"
                  /></el-form-item>
                  <el-form-item :label="t('zpaySiteName')"
                    ><el-input v-model="paymentForm.siteName"
                  /></el-form-item>
                  <el-form-item :label="t('zpayMerchantId')"
                    ><el-input v-model="paymentForm.merchantId"
                  /></el-form-item>
                  <el-form-item :label="t('zpaySecretKey')"
                    ><el-input v-model="paymentForm.secretKey" show-password
                  /></el-form-item>
                </div>
              </div>
            </template>
          </div>

          <div v-else-if="currentBusinessStep === 'upstream'" class="setup-section upstream-setup-section">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><Tickets /></el-icon>
              </span>
              <div>
                <h2>{{ t('upstreamChannels') }}</h2>
                <p>{{ t('setupUpstreamHint') }}</p>
              </div>
            </div>
            <div class="setup-grid upstream-basic-grid">
              <el-form-item :label="t('provider')">
                <el-select v-model="setupForm.provider" filterable>
                  <el-option
                    v-for="provider in providers"
                    :key="provider.code"
                    :label="provider.display_name"
                    :value="provider.code"
                  >
                    <span class="provider-option"
                      ><ProviderIcon :provider="provider.code" />{{ provider.display_name }}</span
                    >
                  </el-option>
                </el-select>
              </el-form-item>
              <el-form-item :label="t('name')">
                <el-input
                  v-model="setupForm.channelName"
                  :placeholder="t('channelNamePlaceholder')"
                />
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
                <el-button
                  :disabled="
                    saving ||
                    configuringPrices ||
                    !setupForm.baseUrl.trim() ||
                    !setupForm.secret.trim()
                  "
                  :icon="Refresh"
                  :loading="fetchingModels"
                  @click="fetchModels"
                >
                  {{ t('autoFetch') }}
                </el-button>
              </div>
            </el-form-item>
          </div>

          <div v-else-if="currentBusinessStep === 'smtp'" class="setup-section compact">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><Message /></el-icon>
              </span>
              <div>
                <h2>{{ t('smtpSettings') }}</h2>
                <p>{{ t('setupSmtpHint') }}</p>
              </div>
            </div>

            <div class="setup-smtp-groups">
              <section class="setup-mini-section">
                <header class="setup-mini-section-header">
                  <el-icon><Connection /></el-icon>
                  <h3>{{ t('smtpConnectionSettings') }}</h3>
                </header>
                <div class="setup-grid smtp-connection-grid">
                  <el-form-item class="smtp-standard-field" :label="t('smtpHost')">
                    <el-input
                      v-model="smtpForm.host"
                      autocomplete="off"
                      :placeholder="t('smtpHostPlaceholder')"
                    />
                  </el-form-item>
                  <el-form-item class="smtp-standard-field" :label="t('smtpPort')">
                    <el-input
                      v-model="smtpPortInput"
                      autocomplete="off"
                      inputmode="numeric"
                      placeholder="587"
                    />
                  </el-form-item>
                  <el-form-item class="smtp-switch-field" :label="t('smtpTls')">
                    <el-switch v-model="smtpForm.tls" />
                  </el-form-item>
                </div>
              </section>

              <section class="setup-mini-section">
                <header class="setup-mini-section-header">
                  <el-icon><Lock /></el-icon>
                  <h3>{{ t('smtpAuthSettings') }}</h3>
                </header>
                <div class="setup-grid smtp-auth-grid">
                  <el-form-item class="smtp-standard-field" :label="t('smtpUsername')">
                    <el-input
                      v-model="smtpForm.username"
                      autocomplete="off"
                      :placeholder="t('smtpUsernamePlaceholder')"
                    />
                  </el-form-item>
                  <el-form-item class="smtp-standard-field" :label="t('smtpPassword')">
                    <el-input
                      v-model="smtpForm.password"
                      autocomplete="new-password"
                      :placeholder="t('smtpPasswordPlaceholder')"
                      show-password
                      type="password"
                    />
                  </el-form-item>
                </div>
              </section>

              <section class="setup-mini-section">
                <header class="setup-mini-section-header">
                  <el-icon><Message /></el-icon>
                  <h3>{{ t('smtpSenderSettings') }}</h3>
                </header>
                <div class="setup-grid smtp-sender-grid">
                  <el-form-item class="smtp-standard-field" :label="t('mailFromEmail')">
                    <el-input
                      v-model="smtpForm.fromEmail"
                      autocomplete="off"
                      :placeholder="t('mailFromEmailPlaceholder')"
                      type="email"
                    />
                  </el-form-item>
                  <el-form-item class="smtp-standard-field" :label="t('mailFromName')">
                    <el-input
                      v-model="smtpForm.fromName"
                      autocomplete="off"
                      :placeholder="t('mailFromNamePlaceholder')"
                    />
                  </el-form-item>
                  <el-form-item class="smtp-standard-field" :label="t('mailSubjectPrefix')">
                    <el-input
                      v-model="smtpForm.subjectPrefix"
                      autocomplete="off"
                      :placeholder="t('mailSubjectPrefixPlaceholder')"
                    />
                  </el-form-item>
                </div>
              </section>
            </div>

            <div class="setup-field-actions smtp-test-actions">
              <el-button
                :icon="Message"
                :loading="testingSmtp"
                :disabled="saving"
                @click="sendSmtpTestEmail"
              >
                {{ t('sendSmtpTestEmail') }}
              </el-button>
            </div>
          </div>

          <div class="setup-actions sticky">
            <el-button
              v-if="currentBusinessStep !== 'admin-password' || canReviewRuntimeConfig"
              :icon="ArrowLeft"
              :disabled="saving"
              @click="goToPreviousBusinessStep"
            >
              {{ t('previousStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep === 'upstream'"
              :disabled="saving || fetchingModels || configuringPrices"
              @click="skipUpstreamStep"
            >
              {{ t('skipStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep !== 'smtp'"
              type="primary"
              :icon="ArrowRight"
              :loading="currentBusinessStep === 'upstream' && configuringPrices"
              :disabled="saving || fetchingModels || configuringPrices"
              @click="goToNextBusinessStep"
            >
              {{ t('nextStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep === 'smtp'"
              :loading="saving && !smtpForm.enabled"
              :type="smtpForm.enabled ? undefined : 'primary'"
              :disabled="saving && smtpForm.enabled"
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
              :disabled="saving"
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
  --setup-footer-height: 38px;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.9), rgba(244, 248, 252, 0.96)),
    linear-gradient(135deg, rgba(22, 139, 211, 0.1), rgba(5, 150, 105, 0.05) 46%, transparent 46%),
    #f6f8fb;
  min-height: calc(100dvh - var(--setup-footer-height));
  overflow: hidden;
  padding: 22px clamp(18px, 3vw, 34px);
  position: relative;
}

.setup-language {
  position: fixed;
  right: clamp(18px, 3vw, 34px);
  top: 18px;
  z-index: 2;
}

.setup-stage {
  align-items: start;
  display: grid;
  gap: clamp(20px, 3vw, 34px);
  grid-template-columns: 300px minmax(0, 680px);
  margin: 0 auto;
  max-width: 1040px;
  min-height: calc(100dvh - var(--setup-footer-height) - 44px);
  padding-top: 24px;
}

.setup-brief {
  align-content: start;
  display: grid;
  gap: 16px;
  padding: 18px 0 0;
  position: sticky;
  top: 18px;
}

.setup-logo-lockup {
  align-items: center;
  display: flex;
  gap: 12px;
}

.setup-logo-lockup img {
  height: 34px;
  object-fit: contain;
  width: 124px;
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
  gap: 10px;
}

.setup-heading h1,
.setup-panel h2 {
  color: #111827;
  letter-spacing: 0;
  margin: 0;
}

.setup-heading h1 {
  font-size: 36px;
  font-weight: 840;
  line-height: 1.12;
}

.setup-heading p,
.setup-panel p {
  color: #64748b;
  font-size: 14px;
  line-height: 1.6;
  margin: 0;
}

.setup-progress-card {
  background: rgba(255, 255, 255, 0.68);
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  display: grid;
  gap: 9px;
  padding: 12px;
}

.setup-progress-card > div {
  align-items: center;
  display: flex;
  justify-content: space-between;
}

.setup-progress-card span {
  color: #64748b;
  font-size: 13px;
  font-weight: 760;
}

.setup-progress-card strong {
  color: #172033;
  font-size: 14px;
  font-weight: 820;
}

.setup-progress-track {
  background: #e6edf5;
  border-radius: 999px;
  display: block;
  height: 8px;
  overflow: hidden;
}

.setup-progress-fill {
  background: #168bd3;
  border-radius: inherit;
  display: block;
  height: 100%;
  transition: width 0.18s ease;
}

.setup-steps {
  display: grid;
  gap: 8px;
  list-style: none;
  margin: 0;
  padding: 0;
}

.setup-steps li {
  align-items: start;
  background: rgba(255, 255, 255, 0.58);
  border: 1px solid transparent;
  border-radius: 8px;
  display: grid;
  gap: 9px;
  grid-template-columns: 26px minmax(0, 1fr);
  padding: 9px;
  transition:
    background-color 0.15s ease,
    border-color 0.15s ease,
    box-shadow 0.15s ease;
}

.setup-steps li.active {
  background: #ffffff;
  border-color: #c7e5f5;
  box-shadow: 0 12px 28px rgba(22, 139, 211, 0.08);
}

.setup-step-mark {
  align-items: center;
  background: #e6edf5;
  border-radius: 50%;
  color: #7a8798;
  display: inline-flex;
  font-size: 13px;
  font-weight: 800;
  height: 26px;
  justify-content: center;
  position: relative;
  width: 26px;
}

.setup-step-mark .el-icon {
  opacity: 0;
  position: absolute;
}

.setup-step-index {
  line-height: 1;
}

.setup-steps li.done .setup-step-mark {
  background: #dcfce7;
  color: #059669;
}

.setup-steps li.done .setup-step-mark .el-icon {
  opacity: 1;
}

.setup-steps li.done .setup-step-index {
  opacity: 0;
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
  font-size: 13px;
  line-height: 1.46;
  margin-top: 3px;
}

.setup-panel {
  align-content: start;
  background: #ffffff;
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  box-shadow: 0 16px 38px rgba(15, 23, 42, 0.08);
  display: grid;
  gap: 16px;
  margin-bottom: 0;
  min-width: 0;
  padding: 22px;
  width: min(680px, 100%);
}

.setup-panel > .el-form {
  display: grid;
  gap: 16px;
  max-width: none;
  width: 100%;
}

.setup-panel :deep(.el-form-item) {
  margin-bottom: 0;
}

.setup-panel :deep(.el-form-item__label) {
  color: #334155;
  font-size: 13px;
  font-weight: 760;
  line-height: 1.3;
  margin-bottom: 5px;
}

.setup-panel :deep(.el-input__wrapper),
.setup-panel :deep(.el-select__wrapper),
.setup-panel :deep(.el-textarea__inner) {
  border-radius: 8px;
  box-shadow: 0 0 0 1px #d9e2ec inset;
  transition:
    box-shadow 0.15s ease,
    background-color 0.15s ease;
}

.setup-panel :deep(.el-input__wrapper),
.setup-panel :deep(.el-select__wrapper) {
  min-height: 38px;
}

.setup-panel :deep(.el-input__wrapper.is-focus),
.setup-panel :deep(.el-select__wrapper.is-focused),
.setup-panel :deep(.el-textarea__inner:focus) {
  box-shadow: 0 0 0 1px #168bd3 inset;
}

.setup-panel :deep(.el-button) {
  border-radius: 8px;
  font-size: 14px;
  font-weight: 720;
  min-height: 36px;
}

.setup-panel-title,
.setup-section-heading {
  align-items: start;
  display: grid;
  gap: 10px;
  grid-template-columns: 38px minmax(0, 1fr);
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
  height: 38px;
  justify-content: center;
  width: 38px;
}

.setup-title-icon.warning {
  background: #fff7ed;
  border-color: #fed7aa;
  color: #ea580c;
}

.setup-panel h2 {
  font-size: 17px;
  font-weight: 820;
  margin: 0;
}

.setup-section {
  border-top: 1px solid #edf1f5;
  display: grid;
  gap: 12px;
  max-width: none;
  padding-top: 16px;
}

.setup-section:first-child {
  border-top: 0;
  padding-top: 0;
}

.setup-section.compact {
  gap: 10px;
}

.nested-setup-section {
  border-top: 1px solid #edf1f5;
  display: grid;
  gap: 12px;
  padding-top: 14px;
}

.setup-grid {
  display: grid;
  gap: 12px;
  max-width: none;
}

.setup-grid.two {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.setup-grid .el-input-number,
.setup-grid .el-select {
  width: 100%;
}

.admin-credentials-grid {
  grid-template-columns: minmax(0, 420px);
  max-width: 420px;
}

.setup-section > .el-form-item {
  max-width: none;
}

.upstream-setup-section {
  max-width: 560px;
}

.upstream-basic-grid {
  grid-template-columns: minmax(0, 460px);
  max-width: 460px;
}

.upstream-setup-section > .el-form-item {
  max-width: 560px;
}

.upstream-setup-section .models-row {
  max-width: 560px;
}

.setup-mode-grid {
  display: grid;
  gap: 10px;
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
  gap: 10px;
  min-height: 134px;
  padding: 14px;
  position: relative;
  text-align: left;
  transition:
    background-color 0.15s ease,
    border-color 0.15s ease;
}

.setup-mode-card:hover {
  background: #fbfdff;
  border-color: #b7dcf2;
}

.setup-mode-card.active {
  border-color: #168bd3;
  background: #f4fbff;
}

.setup-mode-card:disabled {
  cursor: not-allowed;
  opacity: 0.72;
}

.setup-mode-icon {
  align-items: center;
  background: #eef6fb;
  border-radius: 8px;
  color: #168bd3;
  display: inline-flex;
  height: 36px;
  justify-content: center;
  width: 36px;
}

.setup-mode-copy {
  display: grid;
  gap: 6px;
}

.setup-mode-copy strong {
  font-size: 15px;
  font-weight: 820;
}

.setup-mode-copy span {
  color: #64748b;
  font-size: 13px;
  line-height: 1.5;
}

.setup-mode-check {
  color: #168bd3;
  position: absolute;
  right: 14px;
  top: 14px;
}

.setup-inline-control {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  display: flex;
  gap: 12px;
  justify-content: space-between;
  padding: 12px 14px;
}

.credit-required-control,
.payment-enable-control {
  background: transparent;
}

.setup-password-checks {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.setup-check-pill {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 999px;
  color: #64748b;
  display: inline-flex;
  font-size: 13px;
  font-weight: 740;
  gap: 5px;
  min-height: 28px;
  padding: 5px 10px;
  transition:
    background-color 0.15s ease,
    border-color 0.15s ease,
    color 0.15s ease;
}

.setup-check-pill .el-icon {
  opacity: 0.36;
}

.setup-check-pill.ok {
  background: #ecfdf5;
  border-color: #bbf7d0;
  color: #047857;
}

.setup-check-pill.ok .el-icon {
  opacity: 1;
}

.models-row {
  display: grid;
  gap: 8px;
  grid-template-columns: minmax(0, 1fr) auto;
  max-width: none;
  width: 100%;
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
  max-width: 600px;
}

.provider-option {
  align-items: center;
  display: flex;
  gap: 8px;
}

.optional-grid {
  margin-top: 10px;
}

.setup-smtp-groups {
  --smtp-field-width: 300px;
  display: grid;
  gap: 12px;
  max-width: calc(var(--smtp-field-width) * 2 + 12px + 30px);
}

.setup-mini-section {
  background: transparent;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  display: grid;
  gap: 12px;
  padding: 14px;
}

.setup-mini-section-header {
  align-items: center;
  display: flex;
  gap: 8px;
}

.setup-mini-section-header .el-icon {
  align-items: center;
  background: #eaf6fd;
  border: 1px solid #d3edf9;
  border-radius: 8px;
  color: #168bd3;
  display: inline-flex;
  height: 28px;
  justify-content: center;
  width: 28px;
}

.setup-mini-section-header h3 {
  color: #172033;
  font-size: 14px;
  font-weight: 820;
  line-height: 1.3;
  margin: 0;
}

.smtp-connection-grid {
  align-items: end;
  grid-template-columns: minmax(0, var(--smtp-field-width)) 132px 96px;
}

.smtp-auth-grid,
.smtp-sender-grid {
  grid-template-columns: repeat(2, minmax(0, var(--smtp-field-width)));
}

.smtp-standard-field {
  width: 100%;
}

.smtp-switch-field :deep(.el-form-item__content) {
  min-height: 38px;
}

.smtp-switch-field {
  width: var(--smtp-field-width);
}

.smtp-test-actions {
  margin-top: 0;
}

.setup-actions {
  align-items: center;
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  flex-wrap: wrap;
}

.runtime-actions {
  max-width: none;
  width: 100%;
}

.setup-field-actions {
  align-items: center;
  display: flex;
  justify-content: flex-start;
  margin-top: -4px;
}

.setup-actions.sticky {
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.88), #ffffff 40%);
  border-top: 1px solid #edf1f5;
  bottom: 0;
  margin: 0 -6px -8px;
  padding: 12px 6px 0;
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
  gap: 8px;
  justify-content: space-between;
  max-width: none;
  padding: 11px 12px;
  width: 100%;
}

.setup-env-file span {
  color: #64748b;
  font-size: 13px;
  flex: 0 0 auto;
  font-weight: 760;
}

.setup-env-file code {
  color: #172033;
  font-size: 13px;
  overflow-wrap: anywhere;
  text-align: right;
}

.setup-warning-text {
  background: #fff7ed;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  color: #9a3412 !important;
  padding: 10px 12px;
}

@media (max-width: 980px) {
  .setup-shell {
    overflow: visible;
  }

  .setup-stage {
    grid-template-columns: 1fr;
    min-height: auto;
    max-width: 760px;
    padding-top: 54px;
  }

  .setup-brief {
    gap: 16px;
    padding-top: 0;
    position: static;
  }

  .setup-steps {
    grid-template-columns: repeat(5, minmax(120px, 1fr));
    overflow-x: auto;
    padding-bottom: 2px;
  }

  .setup-steps li {
    min-width: 120px;
  }

  .setup-heading h1 {
    font-size: 32px;
  }

  .setup-panel {
    width: 100%;
  }

  .smtp-auth-grid,
  .smtp-sender-grid {
    grid-template-columns: repeat(2, minmax(0, var(--smtp-field-width)));
  }

  .smtp-connection-grid {
    grid-template-columns: minmax(0, var(--smtp-field-width)) 132px 96px;
  }
}

@media (max-width: 760px) {
  .setup-shell {
    padding: 18px;
  }

  .setup-language {
    position: absolute;
    right: 18px;
    top: 18px;
  }

  .setup-grid.two,
  .setup-mode-grid,
  .setup-steps {
    grid-template-columns: 1fr;
  }

  .smtp-connection-grid,
  .smtp-auth-grid,
  .smtp-sender-grid {
    grid-template-columns: 1fr;
  }

  .smtp-standard-field {
    width: 100%;
  }

  .smtp-switch-field {
    width: 100%;
  }

  .setup-progress-card {
    padding: 11px;
  }

  .models-row {
    grid-template-columns: 1fr;
    max-width: none;
  }

  .setup-panel > .el-form,
  .setup-grid,
  .setup-section,
  .setup-section > .el-form-item,
  .upstream-setup-section,
  .upstream-basic-grid,
  .admin-credentials-grid,
  .setup-textarea {
    max-width: none;
  }

  .setup-inline-control,
  .setup-env-file {
    align-items: stretch;
    flex-direction: column;
  }

  .setup-env-file code {
    text-align: left;
  }

  .setup-actions {
    justify-content: stretch;
  }

  .setup-actions.sticky {
    margin: 4px 0 0;
    padding: 16px 0 0;
    position: static;
  }

  .setup-actions .el-button,
  .setup-field-actions .el-button {
    margin-left: 0;
    width: 100%;
  }

  .setup-panel {
    padding: 20px;
  }

  .setup-panel-title,
  .setup-section-heading {
    grid-template-columns: 38px minmax(0, 1fr);
  }

  .setup-title-icon {
    height: 38px;
    width: 38px;
  }
}
</style>
