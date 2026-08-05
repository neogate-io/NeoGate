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
import LocaleToggleButton from '../../components/common/LocaleToggleButton.vue'
import ModelPickerDialog from '../../components/admin/channels/ModelPickerDialog.vue'
import ProviderIcon from '../../components/common/ProviderIcon.vue'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { setSiteBrand } from '../../composables/useSiteBrand'
import type { PricingTemplate, ProviderRecord } from '../../types/admin'
import { majorToMicroAmount, microAmountToMajor } from '../../utils/format'
import {
  ApiError,
  isNoModelsReturnedError,
  readError,
  readModelFetchError,
  readSmtpTestError
} from '../../utils/errors'
import { sortProvidersForDisplay, splitCommaList } from '../../utils/channel'
import { findPricingTemplate } from '../../utils/pricing'

type Protocol = 'openai' | 'anthropic'
type BusinessSetupStep =
  | 'admin-password'
  | 'service-mode'
  | 'upstream'
  | 'smtp'
  | 'payment'
  | 'finish'
type SetupEndpointPayload = {
  protocol: Protocol
  base_url: string
  models: string[]
  enabled: boolean
}
const optionalBusinessSteps = new Set<BusinessSetupStep>(['upstream', 'smtp', 'payment'])

const router = useRouter()
const { locale, t } = useLocale()
const loading = ref(false)
const saving = ref(false)
const fetchingModels = ref(false)
const modelPickerDialogOpen = ref(false)
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
const includePayment = ref(true)
const reviewingRuntimeConfig = ref(false)
const fetchedModels = ref<string[]>([])
const selectedFetchedModels = ref<string[]>([])
const pricingTemplates = ref<PricingTemplate[]>([])

const bootstrapForm = reactive({
  databaseHost: 'localhost',
  databasePort: 5432,
  databaseName: 'neogate',
  databaseUser: '',
  databasePassword: '',
  databaseSslMode: 'auto',
  siteName: 'NeoGate',
  publicBaseUrl: defaultPublicBaseUrl(),
  billingCurrency: defaultBillingCurrency()
})

const databasePortInput = computed({
  get: () => (bootstrapForm.databasePort > 0 ? String(bootstrapForm.databasePort) : ''),
  set: (value: string) => {
    const digits = value.replace(/\D/g, '').slice(0, 5)
    if (!digits) {
      bootstrapForm.databasePort = 0
      return
    }
    bootstrapForm.databasePort = Math.min(Number(digits), 65535)
  }
})

watch(
  locale,
  (next, prev) => {
    if (status.value?.billing_currency) return
    if ((prev === 'zh-CN' && bootstrapForm.billingCurrency === 'CNY') || (prev === 'en-US' && bootstrapForm.billingCurrency === 'USD')) {
      bootstrapForm.billingCurrency = next === 'zh-CN' ? 'CNY' : 'USD'
    }
  },
  { flush: 'post' }
)

const setupForm = reactive({
  adminUsername: 'admin',
  adminPassword: '',
  confirmPassword: '',
  serviceMode: 'internal' as ServiceMode,
  creditRequired: false,
  registrationEnabled: false,
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
  enabled: true,
  apiUrl: 'https://zpayz.cn/submit.php',
  merchantId: '',
  secretKey: '',
  payType: 'wxpay',
  siteName: 'NeoGate'
})

const prices = ref<
  Array<{
    model: string
    inputPrice: number
    outputPrice: number
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

const setupCreditRequiredDescription = computed(() =>
  setupForm.creditRequired
    ? t('creditRequiredEnabledDescription')
    : t('creditRequiredDisabledDescription')
)
const setupRegistrationDescription = computed(() => {
  if (!setupForm.registrationEnabled) return t('registrationDisabledDescription')
  return setupForm.serviceMode === 'paid'
    ? t('registrationPaidEnabledDescription')
    : t('registrationInternalEnabledDescription')
})
const setupPaymentDescription = computed(() =>
  paymentForm.enabled ? t('setupPaymentEnabledHint') : t('setupPaymentDisabledHint')
)
const shouldConfigureSmtp = computed(() => setupForm.registrationEnabled)
const shouldShowPaymentStep = computed(() => setupForm.serviceMode === 'paid' && paymentForm.enabled)
const shouldConfigurePayment = computed(
  () => shouldShowPaymentStep.value && includePayment.value
)
const setupFinishModeTitle = computed(() =>
  setupForm.serviceMode === 'paid' ? t('setupFinishPaidMode') : t('setupFinishInternalMode')
)
const setupFinishModeDetails = computed(() => [
  {
    key: 'credit',
    value:
      setupForm.serviceMode === 'paid' || setupForm.creditRequired
        ? t('setupFinishCreditRequired')
        : t('setupFinishCreditNotRequired')
  },
  {
    key: 'registration',
    value: setupForm.registrationEnabled
      ? t('setupFinishRegistrationEnabled')
      : t('setupFinishRegistrationDisabled')
  },
  ...(setupForm.serviceMode === 'paid'
    ? [
        {
          key: 'payment-feature',
          value: paymentForm.enabled
            ? t('setupFinishPaymentEnabled')
            : t('setupFinishPaymentDisabled')
        }
      ]
    : [])
])
const setupFinishAddonItems = computed(() => [
  {
    key: 'upstream',
    icon: Tickets,
    label: t('upstreamChannels'),
    value: includeUpstream.value ? t('setupFinishUpstreamConfigured') : t('setupFinishSkipped')
  },
  {
    key: 'smtp',
    icon: Message,
    label: t('smtpSettings'),
    value: shouldConfigureSmtp.value
      ? smtpForm.enabled
        ? t('setupFinishSmtpConfigured')
        : t('setupFinishSkipped')
      : t('setupFinishSmtpNotNeeded')
  },
  {
    key: 'payment',
    icon: CreditCard,
    label: t('paymentSettings'),
    value:
      setupForm.serviceMode === 'paid'
        ? shouldConfigurePayment.value
          ? t('setupFinishPaymentConfigured')
          : paymentForm.enabled
            ? t('setupFinishSkipped')
            : t('setupFinishPaymentDisabled')
        : t('setupFinishPaymentNotNeeded')
  }
])
const businessSetupSteps = computed<BusinessSetupStep[]>(() => [
  'admin-password',
  'service-mode',
  'upstream',
  ...(shouldConfigureSmtp.value ? (['smtp'] as const) : []),
  ...(shouldShowPaymentStep.value ? (['payment'] as const) : []),
  'finish'
])
const currentBusinessStepIndex = computed(() =>
  businessSetupSteps.value.indexOf(currentBusinessStep.value)
)
const isLastBusinessStep = computed(
  () => currentBusinessStepIndex.value === businessSetupSteps.value.length - 1
)
const canSkipCurrentBusinessStep = computed(() =>
  optionalBusinessSteps.has(currentBusinessStep.value)
)

const setupSteps = computed(() => {
  const businessStepMeta: Record<
    BusinessSetupStep,
    {
      title: string
      description: string
    }
  > = {
    'admin-password': {
      title: t('setupStepAdminPassword'),
      description: t('setupStepAdminPasswordDescription')
    },
    'service-mode': {
      title: t('setupStepServiceMode'),
      description: t('setupStepServiceModeDescription')
    },
    upstream: {
      title: t('setupStepUpstream'),
      description: t('setupStepUpstreamDescription')
    },
    smtp: {
      title: t('setupStepSmtp'),
      description: t('setupStepSmtpDescription')
    },
    payment: {
      title: t('setupStepPayment'),
      description: t('setupStepPaymentDescription')
    },
    finish: {
      title: t('setupStepFinish'),
      description: t('setupStepFinishDescription')
    }
  }

  return [
    {
      key: 'runtime',
      title: t('setupStepRuntime'),
      description: t('setupStepRuntimeDescription'),
      done: status.value ? !status.value.bootstrap_required && !reviewingRuntimeConfig.value : false,
      active: Boolean(status.value?.bootstrap_required) || reviewingRuntimeConfig.value
    },
    ...businessSetupSteps.value.map((step) => ({
      key: step,
      title: businessStepMeta[step].title,
      description: businessStepMeta[step].description,
      done: isBusinessStepDone(step),
      active: isBusinessStepActive(step)
    }))
  ]
})

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
const providerOptions = computed(() => sortProvidersForDisplay(providers.value))
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
const hasFetchedModels = computed(() => fetchedModels.value.length > 0)
const allFetchedModelsSelected = computed(
  () => hasFetchedModels.value && selectedFetchedModels.value.length === fetchedModels.value.length
)

watch(
  () => setupForm.provider,
  () => applyProviderDefaults()
)

watch(
  () => setupForm.models,
  () => syncPriceRows()
)

watch(selectedFetchedModels, syncSelectedModelsToInput, { deep: true })

watch(
  () => setupForm.serviceMode,
  (serviceMode) => {
    setupForm.registrationEnabled = serviceMode === 'paid'
    if (serviceMode !== 'paid' && currentBusinessStep.value === 'payment') {
      currentBusinessStep.value = 'service-mode'
    }
  }
)

watch(
  () => setupForm.registrationEnabled,
  (enabled) => {
    if (!enabled && currentBusinessStep.value === 'smtp') {
      currentBusinessStep.value = 'service-mode'
    }
  }
)

watch(
  () => paymentForm.enabled,
  (enabled) => {
    if (enabled) {
      includePayment.value = true
    }
    if (!enabled && currentBusinessStep.value === 'payment') {
      currentBusinessStep.value = 'service-mode'
    }
  }
)

watch(
  () => smtpForm.tls,
  (tls) => {
    smtpForm.port = tls ? 587 : 25
  }
)

async function load() {
  await withLoading(loading, async () => {
    try {
      status.value = await getSetupStatus()
      if (status.value.setup_completed) {
        await router.replace('/login')
        return
      }
      bootstrapForm.siteName = status.value.site_name || bootstrapForm.siteName
      bootstrapForm.publicBaseUrl =
        preferredPublicBaseUrl(status.value.public_base_url) || bootstrapForm.publicBaseUrl
      bootstrapForm.billingCurrency = status.value.billing_currency || bootstrapForm.billingCurrency
      paymentForm.siteName = status.value.site_name || paymentForm.siteName

      if (!status.value.bootstrap_required) {
        providers.value = await getSetupProviders()
        if (providerOptions.value.length > 0 && !selectedProvider.value) {
          setupForm.provider = providerOptions.value[0].code
        }
        applyProviderDefaults()
      }
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function saveBootstrap() {
  if (!validateRuntimeConfig()) return
  await withLoading(saving, async () => {
    try {
      const result = await bootstrapSetup({
        database_url:
          bootstrapMissingDatabase.value || reviewingRuntimeConfig.value
            ? buildDatabaseUrl(false)
            : null,
        site_name: bootstrapForm.siteName,
        public_base_url: bootstrapForm.publicBaseUrl,
        billing_currency: bootstrapForm.billingCurrency
      })
      envFile.value = result.restart_required ? result.env_file : ''
      waitingForRestart.value = result.restart_required
      restartWaitTimedOut.value = false
      ElMessage.success(t('runtimeConfigSaved'))
      if (!result.restart_required) {
        status.value = await getSetupStatus(true)
        reviewingRuntimeConfig.value = false
        providers.value = await getSetupProviders()
        if (providerOptions.value.length > 0 && !selectedProvider.value) {
          setupForm.provider = providerOptions.value[0].code
        }
        applyProviderDefaults()
        return
      }
      if (reviewingRuntimeConfig.value) {
        waitingForRestart.value = false
        return
      }
      await waitForRuntimeRestart()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
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
      const nextStatus = await getSetupStatus(true)
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
        if (providerOptions.value.length > 0 && !selectedProvider.value) {
          setupForm.provider = providerOptions.value[0].code
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
  await withLoading(testingDatabase, async () => {
    try {
      await testSetupDatabase({ database_url: buildDatabaseUrl(false) })
      ElMessage.success(t('databaseConnectionSucceeded'))
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function generateClusterTemplate() {
  await withLoading(generatingTemplate, async () => {
    try {
      clusterEnvTemplate.value = (await getClusterEnvTemplate()).env_text
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function fetchModels() {
  const endpoint = setupModelFetchEndpoint()
  if (!endpoint) {
    ElMessage.error(t('baseUrlRequired'))
    return
  }
  if (!setupForm.secret.trim()) {
    ElMessage.error(t('upstreamKeyRequired'))
    return
  }
  const shouldKeepAllSelected = allFetchedModelsSelected.value
  const existingModels = splitCommaList(setupForm.models)

  await withLoading(fetchingModels, async () => {
    try {
      const result = await fetchSetupUpstreamModels({
        provider: setupForm.provider,
        protocol: endpoint.protocol,
        base_url: endpoint.base_url,
        secret: setupForm.secret
      })

      fetchedModels.value = result.models
      selectedFetchedModels.value =
        shouldKeepAllSelected || existingModels.length === 0
          ? result.models
          : result.models.filter((model) => existingModels.includes(model))
      syncSelectedModelsToInput()
      modelPickerDialogOpen.value = true
      if (result.models.length === 0) {
        ElMessage.warning(t('modelsFetchEmpty'))
      } else {
        ElMessage.success(t('modelsFetched'))
      }
    } catch (err) {
      if (isNoModelsReturnedError(err)) {
        fetchedModels.value = existingModels
        selectedFetchedModels.value = existingModels
        modelPickerDialogOpen.value = true
        ElMessage.warning(t('modelsFetchEmpty'))
        return
      }
      ElMessage.error(readModelFetchError(err, t))
    }
  })
}

function toggleAllFetchedModels(checked: boolean) {
  selectedFetchedModels.value = checked ? [...fetchedModels.value] : []
  syncSelectedModelsToInput()
}

function syncSelectedModelsToInput() {
  if (!hasFetchedModels.value) return
  setupForm.models = selectedFetchedModels.value.join(', ')
  syncPriceRows()
}

async function syncAndApplyReferencePrices() {
  try {
    const { templates } = await syncSetupPricingTemplates()
    pricingTemplates.value = templates
    let applied = 0
    for (const price of prices.value) {
      const template = findPricingTemplate(templates, setupForm.provider, price.model)
      price.enabled = Boolean(template)
      if (!template) continue
      price.inputPrice = microAmountToMajor(template.input_price_micros)
      price.outputPrice = microAmountToMajor(template.output_price_micros)
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

  return withLoading(configuringPrices, syncAndApplyReferencePrices)
}

async function submitSetup() {
  if (!validateSetup()) return
  await withLoading(saving, async () => {
    try {
      const models = splitCommaList(setupForm.models)
      const channel = includeUpstream.value
        ? {
            provider: setupForm.provider,
            name: setupForm.channelName.trim(),
            endpoints: setupEndpointsForSubmit(models),
            secret: setupForm.secret
          }
        : null
      const completedStatus = await completeSetupWizard({
        admin_username: setupForm.adminUsername.trim(),
        admin_password: setupForm.adminPassword,
        service_mode: setupForm.serviceMode,
        credit_required: setupForm.serviceMode === 'internal' ? setupForm.creditRequired : true,
        registration_enabled: setupForm.registrationEnabled,
        channel,
        prices: includeUpstream.value
          ? prices.value.map((price) => ({
              provider: setupForm.provider,
              model: price.model,
              input_price_micros: majorToMicroAmount(price.inputPrice),
              output_price_micros: majorToMicroAmount(price.outputPrice),
              enabled: price.enabled
            }))
          : [],
        smtp: shouldConfigureSmtp.value && smtpForm.enabled ? smtpPayload() : null,
        payment:
          shouldConfigurePayment.value
              ? {
                payment_enabled: true,
                zpay_api_url: paymentForm.apiUrl,
                zpay_merchant_id: paymentForm.merchantId || null,
                zpay_secret_key: paymentForm.secretKey || null,
                clear_zpay_secret_key: false,
                zpay_default_pay_type: paymentForm.payType,
                zpay_site_name: paymentForm.siteName
              }
            : null
      })
      setSiteBrand({
        site_name: completedStatus.site_name || bootstrapForm.siteName,
        public_base_url: completedStatus.public_base_url || bootstrapForm.publicBaseUrl,
        logo_url: null,
        billing_currency: completedStatus.billing_currency,
        env_write_supported: completedStatus.env_write_supported
      })
      ElMessage.success(t('setupCompleted'))
      await router.replace('/login')
    } catch (err) {
      ElMessage.error(readError(err))
      await load()
    }
  })
}

function validateSetup() {
  if (!validateAdminStep()) return false
  if (includeUpstream.value && !validateUpstreamStep()) return false
  if (shouldConfigureSmtp.value && smtpForm.enabled && !validateSmtpStep()) return false
  if (shouldConfigurePayment.value && !validatePaymentFields()) return false
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
  if (!bootstrapMissingDatabase.value && !reviewingRuntimeConfig.value) return true
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
  if (!setupForm.secret.trim()) {
    ElMessage.error(t('upstreamKeyRequired'))
    return false
  }
  const models = splitCommaList(setupForm.models)
  if (models.length === 0) {
    ElMessage.error(t('modelsFetchRequired'))
    return false
  }
  const endpoints = setupEndpointsForSubmit(models)
  if (endpoints.length === 0) {
    ElMessage.error(t('baseUrlRequired'))
    return false
  }
  if (endpoints.some((endpoint) => !isValidHttpUrl(endpoint.base_url))) {
    ElMessage.error(t('baseUrlInvalid'))
    return false
  }
  return true
}

function validateSmtpStep() {
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

function validatePaymentFields() {
  if (!isValidHttpUrl(paymentForm.apiUrl.trim())) {
    ElMessage.error(t('zpayApiUrlRequired'))
    return false
  }
  if (!paymentForm.merchantId.trim()) {
    ElMessage.error(t('zpayMerchantIdRequired'))
    return false
  }
  if (!paymentForm.secretKey.trim()) {
    ElMessage.error(t('zpaySecretKeyRequired'))
    return false
  }
  if (!paymentForm.siteName.trim()) {
    ElMessage.error(t('zpaySiteNameRequired'))
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
  await withLoading(testingSmtp, async () => {
    try {
      await testSetupSmtpSetting(smtpPayload())
      ElMessage.success(t('smtpTestEmailSent'))
    } catch (err) {
      ElMessage.error(readSmtpTestError(err, t))
    }
  })
}

function readReferenceSyncError(err: unknown) {
  if (
    err instanceof ApiError &&
    err.code === 'pricing_reference_source_unavailable'
  ) {
    return t('referencePricesSourceUnavailable')
  }

  return readError(err)
}

async function goToNextBusinessStep() {
  if (currentBusinessStep.value === 'admin-password' && !validateAdminStep()) return
  if (currentBusinessStep.value === 'service-mode') {
    includePayment.value = setupForm.serviceMode === 'paid' && paymentForm.enabled
  }
  if (currentBusinessStep.value === 'upstream' && !validateUpstreamStep()) return
  if (currentBusinessStep.value === 'upstream') {
    if (!(await prepareUpstreamPrices())) return
    includeUpstream.value = true
  }
  if (currentBusinessStep.value === 'smtp') {
    if (!validateSmtpStep()) return
    smtpForm.enabled = true
  }
  if (currentBusinessStep.value === 'payment' && !validatePaymentFields()) return
  if (currentBusinessStep.value === 'payment') {
    includePayment.value = true
  }
  goToAdjacentBusinessStep(1)
}

function goToPreviousBusinessStep() {
  if (currentBusinessStep.value === 'admin-password' && canReviewRuntimeConfig.value) {
    reviewingRuntimeConfig.value = true
    return
  }
  goToAdjacentBusinessStep(-1)
}

function skipUpstreamStep() {
  includeUpstream.value = false
  goToAdjacentBusinessStep(1)
}

function skipSmtpStep() {
  smtpForm.enabled = false
  goToAdjacentBusinessStep(1)
}

function skipPaymentStep() {
  goToAdjacentBusinessStep(1)
  includePayment.value = false
}

function skipOptionalBusinessStep() {
  if (currentBusinessStep.value === 'upstream') {
    skipUpstreamStep()
    return
  }
  if (currentBusinessStep.value === 'smtp') {
    skipSmtpStep()
    return
  }
  if (currentBusinessStep.value === 'payment') {
    skipPaymentStep()
  }
}

function goToAdjacentBusinessStep(offset: -1 | 1) {
  const nextStep = businessSetupSteps.value[currentBusinessStepIndex.value + offset]
  if (nextStep) currentBusinessStep.value = nextStep
}

async function handleBusinessSubmit() {
  if (isLastBusinessStep.value) {
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
  return currentBusinessStepIndex.value > businessSetupSteps.value.indexOf(step)
}

function applyProviderDefaults() {
  const provider = selectedProvider.value
  if (!provider) return

  setupForm.channelName = provider.display_name
  const defaultEndpoint =
    provider.default_endpoints.find((item) => item.protocol === 'openai' && item.base_url) ??
    provider.default_endpoints.find((item) => item.protocol === 'anthropic' && item.base_url)
  setupForm.protocol =
    defaultEndpoint?.protocol === 'anthropic' || provider.code === 'anthropic'
      ? 'anthropic'
      : 'openai'
  setupForm.baseUrl = defaultEndpoint?.base_url || ''
}

function setupEndpointsForSubmit(models: string[]) {
  const provider = selectedProvider.value
  const endpointModels = [...models]
  if (!provider) return []

  const endpoints: SetupEndpointPayload[] = []
  for (const endpoint of provider.default_endpoints) {
    if (endpoint.protocol !== 'openai' && endpoint.protocol !== 'anthropic') continue
    const baseUrl =
      endpoint.protocol === setupForm.protocol ? setupForm.baseUrl.trim() : endpoint.base_url.trim()
    if (!baseUrl) continue
    endpoints.push({
      protocol: endpoint.protocol,
      base_url: baseUrl,
      models: endpointModels,
      enabled: true
    })
  }
  return endpoints
}

function setupModelFetchEndpoint() {
  const baseUrl = setupForm.baseUrl.trim()
  return baseUrl
    ? {
        protocol: setupForm.protocol,
        base_url: baseUrl
      }
    : null
}

function isValidHttpUrl(value: string) {
  try {
    const url = new URL(value)
    return url.protocol === 'http:' || url.protocol === 'https:'
  } catch {
    return false
  }
}

function syncPriceRows() {
  const existing = new Map(prices.value.map((price) => [price.model.trim().toLowerCase(), price]))
  prices.value = splitCommaList(setupForm.models).map((model) => ({
    model,
    inputPrice: existing.get(model.trim().toLowerCase())?.inputPrice ?? 0,
    outputPrice: existing.get(model.trim().toLowerCase())?.outputPrice ?? 0,
    enabled:
      existing.get(model.trim().toLowerCase())?.enabled ??
      Boolean(findPricingTemplate(pricingTemplates.value, setupForm.provider, model))
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

function defaultBillingCurrency(): 'USD' | 'CNY' {
  return 'CNY'
}

function preferredPublicBaseUrl(value?: string | null) {
  const browserOrigin = defaultPublicBaseUrl()
  if (!value) return browserOrigin
  if (isLoopbackUrl(value) && !isLoopbackUrl(browserOrigin)) return browserOrigin
  return value
}

function isLoopbackUrl(value: string) {
  try {
    const url = new URL(value)
    return ['localhost', '127.0.0.1', '0.0.0.0', '::1', '[::1]'].includes(url.hostname)
  } catch {
    return false
  }
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
          <img src="/logos/logo.svg" alt="NeoGate" />
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
          <li :class="{ ok: status?.site_configured }">PUBLIC_BASE_URL</li>
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
              <el-form-item :label="t('billingCurrency')">
                <el-select v-model="bootstrapForm.billingCurrency">
                  <el-option :label="t('currencyUsdLabel')" value="USD" />
                  <el-option :label="t('currencyCnyLabel')" value="CNY" />
                </el-select>
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
            <div class="setup-grid two">
              <el-form-item :label="t('databaseHostLabel')">
                <el-input v-model="bootstrapForm.databaseHost" />
              </el-form-item>
              <el-form-item class="setup-database-port-field" :label="t('databasePortLabel')">
                <el-input
                  v-model="databasePortInput"
                  autocomplete="off"
                  inputmode="numeric"
                  placeholder="5432"
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
              <el-button
                :disabled="saving"
                :loading="testingDatabase"
                @click="testDatabaseConnection"
              >
                {{ t('testDatabaseConnection') }}
              </el-button>
            </div>
          </div>

          <div class="setup-actions runtime-actions">
            <el-button
              type="primary"
              :icon="ArrowRight"
              :disabled="testingDatabase || waitingForRestart"
              :loading="saving"
              native-type="submit"
            >
              {{ t('nextStep') }}
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
            <div class="setup-inline-control registration-enable-control">
              <span>
                <strong>{{ t('registrationEnabled') }}</strong>
                <small>{{ setupRegistrationDescription }}</small>
              </span>
              <el-switch v-model="setupForm.registrationEnabled" />
            </div>
            <div
              v-if="setupForm.serviceMode === 'internal'"
              class="setup-inline-control credit-required-control"
            >
              <span>
                <strong>{{ t('creditRequired') }}</strong>
                <small>{{ setupCreditRequiredDescription }}</small>
              </span>
              <el-switch v-model="setupForm.creditRequired" />
            </div>
            <div
              v-if="setupForm.serviceMode === 'paid'"
              class="setup-inline-control payment-enable-control"
            >
              <span>
                <strong>{{ t('paymentGatewayEnabled') }}</strong>
                <small>{{ setupPaymentDescription }}</small>
              </span>
              <el-switch v-model="paymentForm.enabled" />
            </div>
          </div>

          <div
            v-else-if="currentBusinessStep === 'upstream'"
            class="setup-section upstream-setup-section"
          >
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
                    v-for="provider in providerOptions"
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
                    !setupModelFetchEndpoint() ||
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

          <div v-else-if="currentBusinessStep === 'payment'" class="setup-section compact">
            <div class="setup-section-heading">
              <span class="setup-title-icon">
                <el-icon><CreditCard /></el-icon>
              </span>
              <div>
                <h2>{{ t('paymentSettings') }}</h2>
                <p>{{ t('setupPaymentHint') }}</p>
              </div>
            </div>

            <div class="setup-payment-groups">
              <section class="setup-mini-section">
                <header class="setup-mini-section-header">
                  <el-icon><Lock /></el-icon>
                  <h3>{{ t('zpaySettings') }}</h3>
                </header>
                <div class="setup-grid payment-zpay-grid">
                  <el-form-item class="payment-api-field" :label="t('zpayApiUrl')">
                    <el-input
                      v-model="paymentForm.apiUrl"
                      autocomplete="off"
                      :placeholder="t('zpayApiUrlPlaceholder')"
                    />
                  </el-form-item>
                  <el-form-item :label="t('zpaySiteName')">
                    <el-input
                      v-model="paymentForm.siteName"
                      autocomplete="off"
                      :placeholder="t('zpaySiteNamePlaceholder')"
                    />
                  </el-form-item>
                  <el-form-item :label="t('zpayDefaultPayType')">
                    <el-select v-model="paymentForm.payType">
                      <el-option :label="t('wechatPay')" value="wxpay" />
                      <el-option :label="t('alipay')" value="alipay" />
                    </el-select>
                  </el-form-item>
                  <el-form-item :label="t('zpayMerchantId')">
                    <el-input
                      v-model="paymentForm.merchantId"
                      autocomplete="off"
                      :placeholder="t('zpayMerchantIdPlaceholder')"
                    />
                  </el-form-item>
                  <el-form-item :label="t('zpaySecretKey')">
                    <el-input
                      v-model="paymentForm.secretKey"
                      autocomplete="new-password"
                      :placeholder="t('zpaySecretKeyPlaceholder')"
                      show-password
                      type="password"
                    />
                  </el-form-item>
                </div>
              </section>
            </div>
          </div>

          <div v-else-if="currentBusinessStep === 'finish'" class="setup-section compact">
            <div class="setup-finish-header">
              <div>
                <h2>{{ t('setupFinishTitle') }}</h2>
                <p>{{ t('setupFinishHint') }}</p>
              </div>
            </div>

            <section class="setup-finish-mode-card">
              <span class="setup-finish-mode-icon">
                <el-icon><CreditCard v-if="setupForm.serviceMode === 'paid'" /><Briefcase v-else /></el-icon>
              </span>
              <div class="setup-finish-mode-copy">
                <span class="setup-finish-eyebrow">{{ t('serviceMode') }}</span>
                <h3>{{ setupFinishModeTitle }}</h3>
                <div class="setup-finish-mode-details">
                  <span v-for="item in setupFinishModeDetails" :key="item.key">
                    {{ item.value }}
                  </span>
                </div>
              </div>
            </section>

            <div class="setup-finish-addon-list">
              <div v-for="item in setupFinishAddonItems" :key="item.key" class="setup-finish-addon-row">
                <span class="setup-finish-addon-icon">
                  <el-icon><component :is="item.icon" /></el-icon>
                </span>
                <div>
                  <span class="setup-finish-label">{{ item.label }}</span>
                  <span class="setup-finish-addon-value">{{ item.value }}</span>
                </div>
              </div>
            </div>

            <p class="setup-finish-note">
              {{ t('setupFinishDescriptionLong') }}
            </p>
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
              v-if="canSkipCurrentBusinessStep"
              :disabled="saving || fetchingModels || configuringPrices"
              @click="skipOptionalBusinessStep"
            >
              {{ t('skipStep') }}
            </el-button>
            <el-button
              v-if="!isLastBusinessStep"
              type="primary"
              :icon="ArrowRight"
              :loading="currentBusinessStep === 'upstream' && configuringPrices"
              :disabled="saving || fetchingModels || configuringPrices"
              @click="goToNextBusinessStep"
            >
              {{ t('nextStep') }}
            </el-button>
            <el-button
              v-if="currentBusinessStep === 'finish'"
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

    <ModelPickerDialog
      v-model:open="modelPickerDialogOpen"
      v-model:models="fetchedModels"
      v-model:selected-models="selectedFetchedModels"
      :all-selected="allFetchedModelsSelected"
      @toggle-all="toggleAllFetchedModels"
    />
  </main>
</template>

<style scoped>
.setup-shell {
  --setup-footer-height: 38px;
  --setup-panel-height: clamp(620px, calc(100dvh - var(--setup-footer-height) - 68px), 760px);
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
  gap: clamp(18px, 2.8vw, 30px);
  grid-template-columns: 288px minmax(0, 680px);
  margin: 0 auto;
  max-width: 1010px;
  min-height: calc(100dvh - var(--setup-footer-height) - 44px);
  padding-top: 24px;
}

.setup-brief {
  align-content: start;
  display: grid;
  gap: 12px;
  padding: 10px 0 0;
  position: sticky;
  top: 18px;
}

.setup-logo-lockup {
  align-items: center;
  display: flex;
  gap: 10px;
}

.setup-logo-lockup img {
  height: 30px;
  object-fit: contain;
  width: 112px;
}

.setup-logo-lockup span {
  border-left: 1px solid #d8e1ec;
  color: #64748b;
  font-size: 13px;
  font-weight: 700;
  line-height: 1;
  padding-left: 10px;
}

.setup-heading {
  display: grid;
  gap: 7px;
}

.setup-heading h1,
.setup-panel h2 {
  color: #111827;
  letter-spacing: 0;
  margin: 0;
}

.setup-heading h1 {
  font-size: 28px;
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

.setup-heading p {
  font-size: 13px;
  line-height: 1.5;
}

.setup-progress-card {
  background: rgba(255, 255, 255, 0.68);
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  display: grid;
  gap: 8px;
  padding: 10px;
}

.setup-progress-card > div {
  align-items: center;
  display: flex;
  justify-content: space-between;
}

.setup-progress-card span {
  color: #64748b;
  font-size: 12px;
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
  height: 7px;
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
  gap: 6px;
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
  gap: 7px;
  grid-template-columns: 22px minmax(0, 1fr);
  padding: 7px;
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
  height: 22px;
  justify-content: center;
  position: relative;
  width: 22px;
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

.setup-steps strong {
  font-size: 13px;
}

.setup-steps small,
.setup-inline-control small {
  color: #738093;
  display: block;
  font-size: 13px;
  line-height: 1.4;
  margin-top: 3px;
}

.setup-steps small {
  font-size: 12px;
  line-height: 1.36;
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
  height: var(--setup-panel-height);
  min-width: 0;
  overflow: hidden;
  padding: 22px;
  width: min(680px, 100%);
}

.setup-panel > .el-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
  height: 100%;
  max-width: none;
  min-height: 100%;
  overflow-x: hidden;
  overflow-y: auto;
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

.setup-database-port-field :deep(.el-form-item__content),
.setup-database-port-field :deep(.el-input) {
  width: 86px;
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
.registration-enable-control,
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

.setup-payment-groups {
  display: grid;
  gap: 12px;
}

.payment-zpay-grid {
  align-items: start;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.payment-api-field {
  grid-column: 1 / -1;
}

.setup-finish-header {
  display: grid;
  gap: 6px;
}

.setup-finish-mode-card {
  align-items: center;
  background: #f8fafc;
  border: 1px solid #dbeafe;
  border-radius: 8px;
  display: grid;
  gap: 14px;
  grid-template-columns: 42px minmax(0, 1fr);
  padding: 14px 16px;
}

.setup-finish-mode-icon,
.setup-finish-addon-icon {
  align-items: center;
  border-radius: 8px;
  display: inline-flex;
  justify-content: center;
}

.setup-finish-mode-icon {
  background: #168bd3;
  color: #ffffff;
  font-size: 21px;
  height: 42px;
  width: 42px;
}

.setup-finish-mode-copy {
  display: grid;
  gap: 8px;
}

.setup-finish-eyebrow,
.setup-finish-label {
  color: #64748b;
  font-size: 12px;
  font-weight: 760;
  line-height: 1.3;
}

.setup-finish-mode-copy h3 {
  color: #172033;
  font-size: 22px;
  font-weight: 860;
  letter-spacing: 0;
  line-height: 1.2;
  margin: 0;
}

.setup-finish-mode-details {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.setup-finish-mode-details span {
  color: #334155;
  font-size: 12px;
  font-weight: 500;
  line-height: 1.35;
  padding-right: 10px;
}

.setup-finish-addon-list {
  border-top: 1px solid #edf1f5;
  display: grid;
  gap: 0;
}

.setup-finish-addon-row {
  align-items: center;
  border-bottom: 1px solid #edf1f5;
  display: grid;
  gap: 12px;
  grid-template-columns: 34px minmax(0, 1fr);
  min-height: 64px;
  padding: 11px 2px;
}

.setup-finish-addon-icon {
  background: #f1f7fb;
  color: #168bd3;
  font-size: 17px;
  height: 32px;
  width: 32px;
}

.setup-finish-addon-row > div {
  display: grid;
  gap: 4px;
}

.setup-finish-addon-value {
  color: #172033;
  font-size: 13px;
  font-weight: 400;
  line-height: 1.35;
}

.setup-finish-note {
  background: #f8fafc;
  border: 1px solid #dbeafe;
  border-radius: 8px;
  color: #334155 !important;
  padding: 12px 14px;
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
  margin: 0;
  margin-top: auto;
  padding: 12px 0 0;
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
    height: auto;
    min-height: auto;
    overflow: visible;
    width: 100%;
  }

  .setup-panel > .el-form {
    height: auto;
    overflow: visible;
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
  .smtp-sender-grid,
  .payment-zpay-grid {
    grid-template-columns: 1fr;
  }

  .setup-finish-mode-card {
    grid-template-columns: 1fr;
  }

  .payment-api-field {
    grid-column: auto;
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
