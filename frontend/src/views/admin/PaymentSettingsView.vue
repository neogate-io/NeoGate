<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { CreditCard, DocumentCopy, Link, Lock, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getPaymentSetting, savePaymentSetting } from '../../api/settings'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { readError } from '../../utils/errors'
import { copyTextWithMessage } from '../../utils/clipboard'

const { t } = useLocale()

const loading = ref(false)
const saving = ref(false)
const secretKeySet = ref(false)
const activeProvider = ref('zpay')

const form = reactive({
  paymentEnabled: false,
  returnBaseUrl: '',
  zpayApiUrl: 'https://zpayz.cn/submit.php',
  zpayMerchantId: '',
  zpaySecretKey: '',
  zpayDefaultPayType: 'wxpay',
  zpaySiteName: 'NeoGate'
})

const zpayConfigured = computed(
  () =>
    Boolean(form.zpayApiUrl.trim()) &&
    Boolean(form.zpayMerchantId.trim()) &&
    (secretKeySet.value || Boolean(form.zpaySecretKey.trim())) &&
    Boolean(form.zpaySiteName.trim())
)
const zpayNotifyUrl = computed(() =>
  form.returnBaseUrl ? `${form.returnBaseUrl.replace(/\/+$/, '')}/api/payments/zpay/notify` : ''
)

function applySetting(setting: Awaited<ReturnType<typeof getPaymentSetting>>) {
  form.paymentEnabled = setting.payment_enabled
  form.returnBaseUrl = setting.return_base_url ?? ''
  form.zpayApiUrl = setting.zpay_api_url || 'https://zpayz.cn/submit.php'
  form.zpayMerchantId = setting.zpay_merchant_id ?? ''
  form.zpaySecretKey = ''
  form.zpayDefaultPayType = setting.zpay_default_pay_type || 'wxpay'
  form.zpaySiteName = setting.zpay_site_name || 'NeoGate'
  secretKeySet.value = setting.zpay_secret_key_set
}

async function load() {
  await withLoading(loading, async () => {
    try {
      applySetting(await getPaymentSetting())
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function save() {
  await withLoading(saving, async () => {
    try {
      const setting = await savePaymentSetting(paymentPayload())
      applySetting(setting)
      ElMessage.success(t('paymentSettingsSaved'))
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

function paymentPayload() {
  return {
    payment_enabled: form.paymentEnabled,
    return_base_url: form.returnBaseUrl || null,
    zpay_api_url: form.zpayApiUrl,
    zpay_merchant_id: form.zpayMerchantId || null,
    zpay_secret_key: form.zpaySecretKey || null,
    clear_zpay_secret_key: false,
    zpay_default_pay_type: form.zpayDefaultPayType,
    zpay_site_name: form.zpaySiteName
  }
}

async function copyZpayNotifyUrl() {
  if (!zpayNotifyUrl.value) return
  await copyTextWithMessage(zpayNotifyUrl.value, t('paymentCallbackUrlCopied'))
}

onMounted(load)
</script>

<template>
  <section v-loading="loading" class="admin-settings-view">
    <el-form
      class="admin-settings-form payment-settings-form"
      label-position="top"
      @submit.prevent="save"
    >
      <div class="admin-settings-body">
        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><CreditCard /></el-icon>
            <h3>{{ t('paymentGatewayStatus') }}</h3>
          </header>

          <div class="admin-settings-grid payment-status-grid">
            <el-form-item class="admin-settings-switch" :label="t('paymentGatewayEnabled')">
              <el-switch v-model="form.paymentEnabled" />
            </el-form-item>
          </div>
        </section>

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('paymentGatewayProvider') }}</h3>
          </header>

          <div class="payment-gateway-layout">
            <nav class="payment-provider-list" :aria-label="t('paymentGatewayProvider')">
              <button
                class="payment-provider-item"
                :class="{ 'is-active': activeProvider === 'zpay' }"
                type="button"
                @click="activeProvider = 'zpay'"
              >
                <span class="payment-provider-mark">Z</span>
                <span class="payment-provider-text">
                  <strong>ZPAY</strong>
                  <span>{{
                    zpayConfigured ? t('paymentConfigured') : t('paymentNotConfigured')
                  }}</span>
                </span>
                <span class="payment-provider-state">
                  {{ activeProvider === 'zpay' ? t('paymentSelected') : '' }}
                </span>
              </button>
            </nav>

            <section v-if="activeProvider === 'zpay'" class="payment-provider-panel">
              <header class="payment-provider-panel-header">
                <div>
                  <span>{{ t('paymentGatewayCurrent') }}</span>
                  <h4>ZPAY</h4>
                </div>
                <span
                  class="payment-provider-badge"
                  :class="{ 'is-ready': zpayConfigured, 'is-missing': !zpayConfigured }"
                >
                  {{ zpayConfigured ? t('paymentConfigured') : t('paymentNotConfigured') }}
                </span>
              </header>

              <div class="admin-settings-grid payment-zpay-grid">
                <el-form-item class="payment-api-field" :label="t('zpayApiUrl')">
                  <el-input
                    v-model="form.zpayApiUrl"
                    autocomplete="off"
                    :placeholder="t('zpayApiUrlPlaceholder')"
                  />
                </el-form-item>

                <el-form-item :label="t('zpayMerchantId')">
                  <el-input
                    v-model="form.zpayMerchantId"
                    autocomplete="off"
                    :placeholder="t('zpayMerchantIdPlaceholder')"
                  />
                </el-form-item>

                <el-form-item class="payment-secret-field" :label="t('zpaySecretKey')">
                  <el-input
                    v-model="form.zpaySecretKey"
                    :prefix-icon="Lock"
                    :placeholder="
                      secretKeySet ? t('zpaySecretKeySet') : t('zpaySecretKeyPlaceholder')
                    "
                    autocomplete="new-password"
                    show-password
                    type="password"
                  />
                </el-form-item>

                <el-form-item :label="t('zpayDefaultPayType')">
                  <el-select v-model="form.zpayDefaultPayType">
                    <el-option :label="t('wechatPay')" value="wxpay" />
                    <el-option :label="t('alipay')" value="alipay" />
                  </el-select>
                </el-form-item>

                <el-form-item :label="t('zpaySiteName')">
                  <el-input
                    v-model="form.zpaySiteName"
                    autocomplete="off"
                    :placeholder="t('zpaySiteNamePlaceholder')"
                  />
                </el-form-item>
              </div>

              <div class="payment-callback-summary">
                <el-icon><Link /></el-icon>
                <div>
                  <span>{{ t('paymentCallbackSettings') }}</span>
                  <strong>
                    {{ zpayNotifyUrl || t('paymentCallbackUrlUnavailable') }}
                  </strong>
                </div>
                <el-tooltip :content="t('copy')" placement="top" :show-after="600">
                  <el-button
                    class="payment-callback-copy"
                    :aria-label="t('copy')"
                    :disabled="!zpayNotifyUrl"
                    :icon="DocumentCopy"
                    @click="copyZpayNotifyUrl"
                  />
                </el-tooltip>
              </div>
            </section>
          </div>
        </section>

        <div class="admin-settings-actions payment-settings-actions">
          <el-button
            class="admin-action-button"
            native-type="submit"
            type="primary"
            :icon="Select"
            :loading="saving"
          >
            {{ t('save') }}
          </el-button>
        </div>
      </div>
    </el-form>
  </section>
</template>

<style scoped>
.payment-settings-form {
  width: min(980px, 100%);
}

.payment-settings-form :deep(.admin-settings-body) {
  padding: 6px 22px 22px;
}

.payment-settings-form :deep(.admin-settings-section) {
  gap: 16px;
  padding: 20px 0 22px;
}

.payment-settings-actions {
  border-top: 0;
  margin-top: 0;
  padding-top: 10px;
}

.payment-status-grid {
  align-items: end;
  grid-template-columns: 150px;
  row-gap: 14px;
}

.payment-zpay-grid {
  align-items: start;
  column-gap: 20px;
  grid-template-columns: repeat(2, minmax(0, 260px));
  row-gap: 14px;
}

.payment-api-field {
  grid-column: 1 / -1;
  max-width: 540px;
}

.payment-gateway-layout {
  align-items: start;
  display: grid;
  gap: 18px;
  grid-template-columns: minmax(180px, 220px) minmax(0, 1fr);
}

.payment-provider-list {
  display: grid;
  gap: 8px;
  min-width: 0;
}

.payment-provider-item {
  align-items: center;
  appearance: none;
  background: #f8fafc;
  border: 1px solid var(--admin-border);
  border-radius: 8px;
  color: var(--admin-text);
  cursor: pointer;
  display: grid;
  gap: 10px;
  grid-template-columns: 34px minmax(0, 1fr);
  min-height: 64px;
  padding: 10px;
  text-align: left;
  transition:
    background-color 140ms ease,
    border-color 140ms ease;
}

.payment-provider-item.is-active {
  background: var(--brand-blue-soft);
  border-color: var(--brand-blue-border);
}

.payment-provider-mark {
  align-items: center;
  background: #ffffff;
  border: 1px solid var(--admin-border);
  border-radius: 7px;
  color: var(--brand-blue);
  display: inline-flex;
  font-size: 15px;
  font-weight: 760;
  height: 34px;
  justify-content: center;
  width: 34px;
}

.payment-provider-text {
  display: grid;
  gap: 5px;
  min-width: 0;
}

.payment-provider-text strong {
  font-size: 14px;
  line-height: 1;
}

.payment-provider-text span {
  color: var(--admin-text-muted);
  font-size: 12px;
  font-weight: 620;
  line-height: 1;
}

.payment-provider-state {
  color: var(--brand-blue);
  font-size: 12px;
  font-weight: 700;
  grid-column: 2;
  line-height: 1;
  min-height: 12px;
}

.payment-provider-panel {
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  display: grid;
  gap: 16px;
  min-width: 0;
  padding: 16px;
}

.payment-provider-panel-header {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
  min-width: 0;
}

.payment-provider-panel-header span {
  color: var(--admin-text-muted);
  display: block;
  font-size: 12px;
  font-weight: 680;
  line-height: 1;
  margin-bottom: 6px;
}

.payment-provider-panel-header h4 {
  color: var(--admin-heading);
  font-size: 16px;
  font-weight: 780;
  line-height: 1;
  margin: 0;
}

.payment-provider-badge {
  border: 1px solid var(--admin-border);
  border-radius: 999px;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 700;
  line-height: 1;
  padding: 7px 10px;
}

.payment-provider-badge.is-ready {
  background: var(--admin-success-bg);
  border-color: var(--admin-success-border);
  color: var(--admin-success);
}

.payment-provider-badge.is-missing {
  background: var(--admin-warning-bg);
  border-color: var(--admin-warning-border);
  color: var(--admin-warning);
}

.payment-callback-summary {
  align-items: start;
  background: var(--admin-surface-muted);
  border: 1px solid var(--admin-border-soft);
  border-radius: 8px;
  display: grid;
  gap: 10px;
  grid-template-columns: 28px minmax(0, 1fr) 34px;
  max-width: 540px;
  padding: 12px;
}

.payment-callback-summary .el-icon {
  align-items: center;
  background: #ffffff;
  border: 1px solid var(--admin-border);
  border-radius: 7px;
  color: var(--brand-blue);
  display: inline-flex;
  height: 28px;
  justify-content: center;
  width: 28px;
}

.payment-callback-summary span {
  color: var(--admin-text-muted);
  display: block;
  font-size: 12px;
  font-weight: 680;
  line-height: 1;
  margin-bottom: 7px;
}

.payment-callback-summary strong {
  color: var(--admin-text);
  display: block;
  font-family:
    ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New',
    monospace;
  font-size: 12px;
  font-weight: 650;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.payment-callback-copy.el-button {
  align-self: center;
  border-radius: 7px;
  height: 34px;
  min-height: 34px;
  padding: 0;
  width: 34px;
}

@media (max-width: 980px) {
  .payment-zpay-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .payment-gateway-layout {
    grid-template-columns: 1fr;
  }

  .payment-provider-list {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .payment-api-field,
  .payment-secret-field {
    grid-column: 1 / -1;
    max-width: none;
  }

  .payment-callback-summary {
    max-width: none;
  }
}

@media (max-width: 640px) {
  .payment-provider-list,
  .payment-zpay-grid {
    grid-template-columns: 1fr;
  }

  .payment-provider-panel {
    padding: 14px;
  }

  .payment-provider-panel-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .payment-callback-summary {
    grid-template-columns: 28px minmax(0, 1fr);
  }

  .payment-callback-copy.el-button {
    grid-column: 2;
    justify-self: start;
  }

  .payment-api-field,
  .payment-secret-field {
    grid-column: auto;
  }
}
</style>
