<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { CreditCard, Link, Lock, Select } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getPaymentSetting, savePaymentSetting } from '../../api/settings'
import { useLocale } from '../../composables/useLocale'
import { readError } from '../../utils/errors'

const { t } = useLocale()

const loading = ref(false)
const saving = ref(false)
const secretKeySet = ref(false)

const form = reactive({
  paymentEnabled: false,
  returnBaseUrl: '',
  zpayApiUrl: 'https://zpayz.cn/submit.php',
  zpayMerchantId: '',
  zpaySecretKey: '',
  zpayDefaultPayType: 'wxpay',
  zpaySiteName: 'NeoGate'
})

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
  loading.value = true
  try {
    applySetting(await getPaymentSetting())
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function save() {
  saving.value = true
  try {
    const setting = await savePaymentSetting(paymentPayload())
    applySetting(setting)
    ElMessage.success(t('paymentSettingsSaved'))
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
  }
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

onMounted(load)
</script>

<template>
  <section v-loading="loading" class="admin-settings-view">
    <el-form class="admin-settings-form" label-position="top" @submit.prevent="save">
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

            <el-form-item :label="t('paymentGatewayProvider')">
              <el-input model-value="ZPAY" disabled />
            </el-form-item>
          </div>
        </section>

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Link /></el-icon>
            <h3>{{ t('paymentCallbackSettings') }}</h3>
          </header>

          <div class="admin-settings-grid payment-callback-grid">
            <el-form-item :label="t('paymentReturnBaseUrl')">
              <el-input
                v-model="form.returnBaseUrl"
                autocomplete="off"
                :placeholder="t('paymentReturnBaseUrlPlaceholder')"
              />
            </el-form-item>
          </div>
        </section>

        <section class="admin-settings-section">
          <header class="admin-settings-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('zpaySettings') }}</h3>
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
                :placeholder="secretKeySet ? t('zpaySecretKeySet') : t('zpaySecretKeyPlaceholder')"
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
        </section>

        <div class="admin-settings-actions">
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
.payment-status-grid {
  align-items: end;
  grid-template-columns: 160px minmax(160px, 220px);
}

.payment-callback-grid {
  grid-template-columns: minmax(320px, 520px);
}

.payment-zpay-grid {
  align-items: start;
  grid-template-columns: minmax(260px, 360px) minmax(220px, 280px);
}

.payment-api-field,
.payment-secret-field {
  grid-column: 1 / -1;
  max-width: 520px;
}

@media (max-width: 980px) {
  .payment-status-grid,
  .payment-zpay-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .payment-callback-grid,
  .payment-api-field,
  .payment-secret-field {
    grid-column: 1 / -1;
    max-width: none;
  }
}

@media (max-width: 640px) {
  .payment-status-grid,
  .payment-zpay-grid {
    grid-template-columns: 1fr;
  }
}
</style>
