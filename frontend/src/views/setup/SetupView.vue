<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Briefcase, Check, CreditCard, Key, Refresh, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  bootstrapSetup,
  completeSetupWizard,
  fetchSetupUpstreamModels,
  getClusterEnvTemplate,
  getSetupProviders,
  getSetupStatus,
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

const router = useRouter()
const { t } = useLocale()
const loading = ref(false)
const saving = ref(false)
const fetchingModels = ref(false)
const generatingTemplate = ref(false)
const status = ref<ServicePolicy | null>(null)
const providers = ref<ProviderRecord[]>([])
const envFile = ref('')
const clusterEnvTemplate = ref('')

const bootstrapForm = reactive({
  setupToken: '',
  databaseUrl: 'postgres://localhost/neogate',
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
const showBusinessSetup = computed(
  () => status.value && !status.value.bootstrap_required && !status.value.setup_completed
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
  if (!bootstrapForm.setupToken.trim()) {
    ElMessage.error('Enter the setup token printed in the backend log.')
    return
  }
  saving.value = true
  try {
    const result = await bootstrapSetup({
      setup_token: bootstrapForm.setupToken,
      database_url: bootstrapMissingDatabase.value ? bootstrapForm.databaseUrl : null,
      site_name: bootstrapForm.siteName,
      public_base_url: bootstrapForm.publicBaseUrl
    })
    envFile.value = result.env_file
    ElMessage.success('Configuration saved. Restart NeoGate to continue.')
    await load()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
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
    await completeSetupWizard({
      admin_password: setupForm.adminPassword,
      service_mode: setupForm.serviceMode,
      credit_required: setupForm.serviceMode === 'internal' ? setupForm.creditRequired : true,
      channel: {
        provider: setupForm.provider,
        name: setupForm.channelName.trim(),
        protocol: setupForm.protocol,
        base_url: setupForm.baseUrl.trim(),
        models: splitCommaList(setupForm.models),
        secret: setupForm.secret
      },
      prices: prices.value.map((price) => ({
        provider: setupForm.provider,
        model: price.model,
        input_price_usd_micros: usdToMicroUsd(price.inputUsd),
        output_price_usd_micros: usdToMicroUsd(price.outputUsd),
        enabled: price.enabled
      })),
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
  if (!setupForm.adminPassword || setupForm.adminPassword.length < 8) {
    ElMessage.error(t('passwordMinLength'))
    return false
  }
  if (setupForm.adminPassword !== setupForm.confirmPassword) {
    ElMessage.error(t('adminPasswordMismatch'))
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

onMounted(load)
</script>

<template>
  <main class="setup-shell">
    <LocaleToggleButton class="setup-language home-language-button" />
    <section v-loading="loading" class="setup-stage">
      <div class="setup-heading">
        <h1>{{ t('setupTitle') }}</h1>
        <p>{{ t('setupSubtitle') }}</p>
      </div>

      <section v-if="status?.restart_required || envFile" class="setup-panel">
        <h2>Restart required</h2>
        <p>Configuration was written to {{ envFile || '.env' }}. Restart NeoGate, then return to this page.</p>
      </section>

      <section v-else-if="clusterBlocked" class="setup-panel">
        <h2>Cluster configuration required</h2>
        <p>Distributed mode must be configured through shared deployment environment variables. This node will not write a local .env file.</p>
        <ul class="setup-check-list">
          <li :class="{ ok: status?.database_configured }">DATABASE_URL</li>
          <li :class="{ ok: status?.redis_configured }">REDIS_URL</li>
          <li :class="{ ok: status?.secrets_configured }">ADMIN_TOKEN_SECRET / UPSTREAM_SECRET_KEY</li>
          <li :class="{ ok: status?.site_configured }">SITE_NAME / PUBLIC_BASE_URL</li>
        </ul>
        <el-button :loading="generatingTemplate" @click="generateClusterTemplate">
          Generate environment template
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

      <section v-else-if="canConfigureEnv" class="setup-panel">
        <h2>Runtime configuration</h2>
        <el-form label-position="top" @submit.prevent="saveBootstrap">
          <el-form-item label="Setup token">
            <el-input v-model="bootstrapForm.setupToken" :prefix-icon="Key" show-password />
          </el-form-item>
          <el-form-item v-if="bootstrapMissingDatabase" label="DATABASE_URL">
            <el-input v-model="bootstrapForm.databaseUrl" />
          </el-form-item>
          <div class="setup-grid two">
            <el-form-item label="SITE_NAME">
              <el-input v-model="bootstrapForm.siteName" />
            </el-form-item>
            <el-form-item label="PUBLIC_BASE_URL">
              <el-input v-model="bootstrapForm.publicBaseUrl" />
            </el-form-item>
          </div>
          <el-button type="primary" :loading="saving" native-type="submit">
            Save runtime configuration
          </el-button>
        </el-form>
      </section>

      <section v-else-if="showBusinessSetup" class="setup-panel">
        <el-form label-position="top" @submit.prevent="submitSetup">
          <h2>{{ t('adminPasswordSettings') }}</h2>
          <div class="setup-grid two">
            <el-form-item :label="t('newPassword')">
              <el-input v-model="setupForm.adminPassword" show-password type="password" />
            </el-form-item>
            <el-form-item :label="t('confirmNewPassword')">
              <el-input v-model="setupForm.confirmPassword" show-password type="password" />
            </el-form-item>
          </div>

          <h2>{{ t('serviceMode') }}</h2>
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
          <el-form-item v-if="setupForm.serviceMode === 'internal'" :label="t('creditRequired')">
            <el-switch v-model="setupForm.creditRequired" />
          </el-form-item>

          <h2>{{ t('upstreamChannels') }}</h2>
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
            <el-form-item label="Protocol">
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

          <h2>{{ t('modelPrices') }}</h2>
          <div class="price-list">
            <div v-for="price in prices" :key="price.model" class="price-row">
              <span>{{ price.model }}</span>
              <el-input-number v-model="price.inputUsd" :min="0" :precision="6" :step="0.1" />
              <el-input-number v-model="price.outputUsd" :min="0" :precision="6" :step="0.1" />
              <el-switch v-model="price.enabled" />
            </div>
          </div>

          <h2>{{ t('smtpSettings') }}</h2>
          <el-switch v-model="smtpForm.enabled" />
          <div v-if="smtpForm.enabled" class="setup-grid two optional-grid">
            <el-form-item :label="t('smtpHost')"><el-input v-model="smtpForm.host" /></el-form-item>
            <el-form-item :label="t('smtpPort')"><el-input-number v-model="smtpForm.port" :min="1" :max="65535" /></el-form-item>
            <el-form-item :label="t('smtpUsername')"><el-input v-model="smtpForm.username" /></el-form-item>
            <el-form-item :label="t('smtpPassword')"><el-input v-model="smtpForm.password" show-password /></el-form-item>
            <el-form-item :label="t('mailFromEmail')"><el-input v-model="smtpForm.fromEmail" /></el-form-item>
            <el-form-item :label="t('mailFromName')"><el-input v-model="smtpForm.fromName" /></el-form-item>
          </div>

          <template v-if="setupForm.serviceMode === 'paid'">
            <h2>{{ t('paymentSettings') }}</h2>
            <el-switch v-model="paymentForm.enabled" />
            <div v-if="paymentForm.enabled" class="setup-grid two optional-grid">
              <el-form-item :label="t('zpayApiUrl')"><el-input v-model="paymentForm.apiUrl" /></el-form-item>
              <el-form-item :label="t('zpaySiteName')"><el-input v-model="paymentForm.siteName" /></el-form-item>
              <el-form-item :label="t('zpayMerchantId')"><el-input v-model="paymentForm.merchantId" /></el-form-item>
              <el-form-item :label="t('zpaySecretKey')"><el-input v-model="paymentForm.secretKey" show-password /></el-form-item>
            </div>
          </template>

          <el-button class="setup-submit" type="primary" :icon="Select" :loading="saving" native-type="submit">
            {{ t('completeSetup') }}
          </el-button>
        </el-form>
      </section>
    </section>
  </main>
</template>

<style scoped>
.setup-shell {
  background: #f6f9fc;
  min-height: 100vh;
  padding: 24px;
  position: relative;
}

.setup-language {
  position: fixed;
  right: 24px;
  top: 24px;
  z-index: 2;
}

.setup-stage {
  display: grid;
  gap: 18px;
  margin: 0 auto;
  max-width: 980px;
  padding-top: 54px;
}

.setup-heading {
  display: grid;
  gap: 8px;
  text-align: center;
}

.setup-heading h1,
.setup-panel h2 {
  color: #111827;
  letter-spacing: 0;
  margin: 0;
}

.setup-heading h1 {
  font-size: 38px;
  font-weight: 840;
}

.setup-heading p,
.setup-panel p {
  color: #64748b;
  font-size: 14px;
  line-height: 1.7;
  margin: 0;
}

.setup-panel {
  background: #ffffff;
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  box-shadow: 0 14px 42px rgba(15, 23, 42, 0.07);
  display: grid;
  gap: 18px;
  padding: 22px;
}

.setup-panel h2 {
  font-size: 18px;
  font-weight: 820;
  margin-top: 8px;
}

.setup-grid {
  display: grid;
  gap: 14px;
}

.setup-grid.two {
  grid-template-columns: repeat(2, minmax(0, 1fr));
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
  gap: 14px;
  min-height: 164px;
  padding: 18px;
  position: relative;
  text-align: left;
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

.price-row + .price-row {
  border-top: 1px solid #e2e8f0;
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

.setup-submit {
  justify-self: start;
  margin-top: 8px;
}

@media (max-width: 760px) {
  .setup-shell {
    padding: 18px;
  }

  .setup-grid.two,
  .setup-mode-grid,
  .price-row {
    grid-template-columns: 1fr;
  }

  .models-row {
    grid-template-columns: 1fr;
  }
}
</style>
