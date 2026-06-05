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
  <section v-loading="loading" class="payment-settings-view">
    <el-form class="payment-settings-form" label-position="top" @submit.prevent="save">
      <div class="payment-settings-body">
        <section class="payment-settings-section">
          <header class="payment-section-header">
            <el-icon><CreditCard /></el-icon>
            <h3>{{ t('paymentGatewayStatus') }}</h3>
          </header>

          <div class="payment-grid payment-status-grid">
            <el-form-item class="payment-switch-field" :label="t('paymentGatewayEnabled')">
              <el-switch v-model="form.paymentEnabled" />
            </el-form-item>

            <el-form-item :label="t('paymentGatewayProvider')">
              <el-input model-value="ZPAY" disabled />
            </el-form-item>
          </div>
        </section>

        <section class="payment-settings-section">
          <header class="payment-section-header">
            <el-icon><Link /></el-icon>
            <h3>{{ t('paymentCallbackSettings') }}</h3>
          </header>

          <div class="payment-grid payment-callback-grid">
            <el-form-item :label="t('paymentReturnBaseUrl')">
              <el-input v-model="form.returnBaseUrl" autocomplete="off" :placeholder="t('paymentReturnBaseUrlPlaceholder')" />
            </el-form-item>
          </div>
        </section>

        <section class="payment-settings-section">
          <header class="payment-section-header">
            <el-icon><Lock /></el-icon>
            <h3>{{ t('zpaySettings') }}</h3>
          </header>

          <div class="payment-grid payment-zpay-grid">
            <el-form-item class="payment-api-field" :label="t('zpayApiUrl')">
              <el-input v-model="form.zpayApiUrl" autocomplete="off" :placeholder="t('zpayApiUrlPlaceholder')" />
            </el-form-item>

            <el-form-item :label="t('zpayMerchantId')">
              <el-input v-model="form.zpayMerchantId" autocomplete="off" :placeholder="t('zpayMerchantIdPlaceholder')" />
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
              <el-input v-model="form.zpaySiteName" autocomplete="off" :placeholder="t('zpaySiteNamePlaceholder')" />
            </el-form-item>
          </div>
        </section>

        <div class="payment-settings-actions">
          <el-button class="admin-action-button" native-type="submit" type="primary" :icon="Select" :loading="saving">
            {{ t('save') }}
          </el-button>
        </div>
      </div>
    </el-form>
  </section>
</template>

<style scoped>
.payment-settings-view {
  display: flex;
  justify-content: flex-start;
  width: 100%;
}

.payment-settings-form {
  background: #fff;
  border: 1px solid #e2e7ef;
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.03);
  overflow: hidden;
  width: min(860px, 100%);
}

.payment-settings-body {
  display: grid;
  gap: 0;
  padding: 4px 18px 18px;
}

.payment-settings-section {
  border-bottom: 1px solid #edf1f5;
  display: grid;
  gap: 14px;
  padding: 18px 0 20px;
}

.payment-section-header {
  align-items: center;
  color: #202b3c;
  display: grid;
  gap: 9px;
  grid-template-columns: auto minmax(0, 1fr);
}

.payment-section-header .el-icon {
  color: var(--brand-blue);
  font-size: 17px;
}

.payment-section-header h3 {
  font-size: 15px;
  font-weight: 760;
  line-height: 1.25;
  margin: 0;
}

.payment-grid {
  display: grid;
  gap: 16px;
  justify-content: start;
}

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

.payment-settings-form :deep(.el-input-number),
.payment-settings-form :deep(.el-select) {
  width: 100%;
}

.payment-settings-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.payment-settings-form :deep(.el-form-item__label) {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 720;
  line-height: 1.2;
  margin-bottom: 8px;
}

.payment-settings-form :deep(.el-input__wrapper),
.payment-settings-form :deep(.el-select__wrapper) {
  border-radius: 7px;
  min-height: 34px;
}

.payment-switch-field :deep(.el-form-item__content) {
  align-items: center;
  min-height: 34px;
  width: max-content;
}

.payment-settings-actions {
  border-top: 1px solid #edf1f5;
  display: flex;
  gap: 10px;
  justify-content: flex-end;
  margin-left: auto;
  margin-top: 18px;
  min-width: max-content;
  padding-top: 18px;
}

@media (max-width: 980px) {
  .payment-settings-form {
    width: 100%;
  }

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
  .payment-settings-actions {
    justify-content: stretch;
    margin-left: 0;
    min-width: 0;
  }

  .payment-settings-actions .el-button {
    flex: 1 1 0;
    min-width: 0;
  }

  .payment-status-grid,
  .payment-zpay-grid {
    grid-template-columns: 1fr;
  }
}
</style>
