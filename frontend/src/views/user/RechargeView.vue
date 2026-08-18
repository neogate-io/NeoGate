<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { Check, CreditCard, Refresh, View } from '@element-plus/icons-vue'
import {
  createRechargeOrder,
  getRechargeOrders,
  type PaymentOrder,
  type PayType
} from '../../api/recharge'
import { useBillingCurrency } from '../../composables/useBillingCurrency'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { readError } from '../../utils/errors'
import { formatDateTime } from '../../utils/format'

const { locale, t } = useLocale()
const { billingCurrency, currencySymbol, formatMoney, majorToMicroAmount } = useBillingCurrency()
const selectedAmount = ref<number | null>(null)
const customAmount = ref<number | null>(null)
const payType = ref<PayType>('wxpay')
const submitting = ref(false)
const historyDialogVisible = ref(false)
const orders = ref<PaymentOrder[]>([])
const ordersLoaded = ref(false)
const loading = ref(false)
const ordersInitialLoading = computed(() => loading.value && !ordersLoaded.value)

const planAmounts = computed(() =>
  billingCurrency.value === 'CNY' ? [2, 10, 20, 50, 200, 500] : [10, 50, 100, 200, 1000, 2000]
)

const recommendedPlanIndex = computed(() => (billingCurrency.value === 'CNY' ? 3 : 2))

const plans = computed(() => [
  { key: 'trial', amount: planAmounts.value[0], name: t('trialPlan'), hint: t('trialPlanHint') },
  {
    key: 'builder',
    amount: planAmounts.value[1],
    name: t('builderPlan'),
    hint: t('builderPlanHint')
  },
  {
    key: 'growth',
    amount: planAmounts.value[2],
    name: t('growthPlan'),
    hint: t('growthPlanHint'),
    recommended: recommendedPlanIndex.value === 2
  },
  {
    key: 'pro',
    amount: planAmounts.value[3],
    name: t('proPlan'),
    hint: t('proPlanHint'),
    recommended: recommendedPlanIndex.value === 3
  },
  {
    key: 'business',
    amount: planAmounts.value[4],
    name: t('businessPlan'),
    hint: t('businessPlanHint')
  },
  {
    key: 'enterprise',
    amount: planAmounts.value[5],
    name: t('enterprisePlan'),
    hint: t('enterprisePlanHint')
  }
])

const defaultPlanAmount = computed(() => planAmounts.value[recommendedPlanIndex.value])

const paymentOptions = computed(() => [
  { label: t('wechatPay'), value: 'wxpay' as PayType, icon: '/icons/wechat-pay.svg' },
  { label: t('alipay'), value: 'alipay' as PayType, icon: '/icons/alipay.svg' }
])

const amountMajor = computed(() => {
  const custom = Number(customAmount.value)
  return Number.isInteger(custom) && custom > 0
    ? custom
    : (selectedAmount.value ?? defaultPlanAmount.value)
})

const amountMicro = computed(() => majorToMicroAmount(amountMajor.value))

const customAmountStyle = computed(() => ({
  '--recharge-currency-symbol': JSON.stringify(currencySymbol.value)
}))

function selectPlan(amount: number) {
  selectedAmount.value = amount
  customAmount.value = null
}

function formatMajorAmount(amount: number) {
  return formatMoney(majorToMicroAmount(amount), locale.value, 0)
}

function formatPayable(order: { payable_amount_minor: number; currency: string }) {
  if (order.currency === 'CNY') return formatCny(order.payable_amount_minor / 100)
  return `${order.currency} ${(order.payable_amount_minor / 100).toFixed(2)}`
}

function formatCny(amount: number) {
  return `¥${amount.toLocaleString(locale.value, {
    minimumFractionDigits: Number.isInteger(amount) ? 0 : 2,
    maximumFractionDigits: 2
  })}`
}

function formatTime(value: string) {
  return formatDateTime(value, locale.value)
}

function orderStatusType(status: string) {
  if (status === 'paid') return 'success'
  if (status === 'failed' || status === 'canceled' || status === 'expired') return 'danger'
  return 'warning'
}

function orderStatusLabel(status: string) {
  if (status === 'paid') return t('paymentStatusPaid')
  if (status === 'failed') return t('paymentStatusFailed')
  if (status === 'canceled') return t('paymentStatusCanceled')
  if (status === 'expired') return t('paymentStatusExpired')
  return t('paymentStatusPending')
}

function paymentReturnUrl() {
  return `${window.location.origin}/payment/return`
}

async function openRechargeHistory() {
  historyDialogVisible.value = true
  if (!ordersLoaded.value) await reloadOrders()
}

async function reloadOrders() {
  await withLoading(loading, async () => {
    try {
      orders.value = await getRechargeOrders()
      ordersLoaded.value = true
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function submitRecharge() {
  if (
    !Number.isFinite(amountMajor.value) ||
    !Number.isInteger(amountMajor.value) ||
    amountMajor.value <= 0
  ) {
    ElMessage.error(t('rechargeAmountRequired'))
    return
  }

  await withLoading(submitting, async () => {
    try {
      const result = await createRechargeOrder(amountMicro.value, payType.value, paymentReturnUrl())
      if (ordersLoaded.value || historyDialogVisible.value) await reloadOrders()
      if (result.checkout_url) {
        window.location.href = result.checkout_url
      } else {
        ElMessage.success(t('rechargeOrderCreated'))
      }
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}
</script>

<template>
  <section class="recharge-view">
    <div class="recharge-layout">
      <div class="user-panel recharge-form-panel">
        <div class="user-section-header">
          <div>
            <h3>{{ t('selectPlan') }}</h3>
          </div>
        </div>

        <div class="plan-card-grid">
          <button
            v-for="plan in plans"
            :key="plan.key"
            class="plan-card"
            :class="{
              active: customAmount == null && amountMajor === plan.amount,
              recommended: plan.recommended
            }"
            type="button"
            @click="selectPlan(plan.amount)"
          >
            <span class="plan-card-title">
              <span class="plan-name">{{ plan.name }}</span>
              <span v-if="plan.recommended" class="plan-badge">{{ t('recommended') }}</span>
            </span>
            <strong>{{ formatMajorAmount(plan.amount) }}</strong>
            <span class="plan-hint">{{ plan.hint }}</span>
            <el-icon v-if="customAmount == null && amountMajor === plan.amount"><Check /></el-icon>
          </button>
        </div>

        <div class="recharge-section recharge-custom-section">
          <h3>{{ t('customAmount') }}</h3>
          <el-input-number
            v-model="customAmount"
            class="custom-amount-input"
            :style="customAmountStyle"
            :min="1"
            :max="9999"
            :step="1"
            :precision="0"
            step-strictly
            :controls="false"
          />
        </div>
      </div>

      <aside class="user-panel recharge-summary-panel">
        <div class="user-section-header">
          <div>
            <h3>{{ t('orderInfo') }}</h3>
          </div>
          <el-button
            class="recharge-history-trigger"
            text
            :icon="View"
            :loading="loading"
            @click="openRechargeHistory"
          >
            {{ t('viewRechargeOrders') }}
          </el-button>
        </div>
        <div class="recharge-summary-total">
          <span>{{ t('payAmount') }}</span>
          <strong>{{ formatMajorAmount(amountMajor) }}</strong>
        </div>
        <dl class="recharge-summary-list">
          <div v-if="billingCurrency !== 'CNY'">
            <dt>{{ t('creditedAmount') }}</dt>
            <dd>{{ formatMajorAmount(amountMajor) }}</dd>
          </div>
          <div>
            <dt>{{ t('paymentMethod') }}</dt>
            <dd>
              <el-segmented v-model="payType" class="payment-methods" :options="paymentOptions">
                <template #default="{ item }">
                  <span class="payment-method-option">
                    <img
                      class="payment-method-icon"
                      :src="item.icon"
                      :alt="item.label"
                      aria-hidden="true"
                    />
                    <span>{{ item.label }}</span>
                  </span>
                </template>
              </el-segmented>
            </dd>
          </div>
        </dl>
        <el-button
          class="recharge-submit"
          type="primary"
          size="large"
          :icon="CreditCard"
          :loading="submitting"
          @click="submitRecharge"
        >
          {{ t('createRechargeOrder') }}
        </el-button>
      </aside>
    </div>

    <el-dialog
      v-model="historyDialogVisible"
      class="recharge-history-dialog"
      :title="t('rechargeOrders')"
      width="min(920px, 92vw)"
    >
      <div class="recharge-history-dialog-body">
        <div class="recharge-dialog-toolbar">
          <el-tooltip :content="t('refresh')" placement="top">
            <el-button :icon="Refresh" :loading="loading" @click="reloadOrders" />
          </el-tooltip>
        </div>
        <div v-if="ordersInitialLoading" class="recharge-order-loading" aria-hidden="true">
          <span v-for="index in 3" :key="index">
            <i></i>
            <i></i>
            <i></i>
          </span>
        </div>
        <el-table
          v-else
          v-loading="loading"
          class="admin-table service-table"
          :data="orders"
          stripe
        >
          <el-table-column label="订单号" min-width="130">
            <template #default="{ row }">{{ row.order_no }}</template>
          </el-table-column>
          <el-table-column :label="t('time')" min-width="170">
            <template #default="{ row }">{{ formatTime(row.created_at) }}</template>
          </el-table-column>
          <el-table-column :label="t('amount')" min-width="110">
            <template #default="{ row }">{{ formatMoney(row.amount_micros, locale, 2) }}</template>
          </el-table-column>
          <el-table-column :label="t('payAmount')" min-width="120">
            <template #default="{ row }">{{ formatPayable(row) }}</template>
          </el-table-column>
          <el-table-column :label="t('status')" width="110" align="center" header-align="center">
            <template #default="{ row }">
              <el-tag :type="orderStatusType(row.status)" effect="plain">
                {{ orderStatusLabel(row.status) }}
              </el-tag>
            </template>
          </el-table-column>
          <template #empty>
            <el-empty :description="t('noData')" />
          </template>
        </el-table>
        <div v-if="ordersInitialLoading" class="recharge-order-cards recharge-order-card-loading">
          <article v-for="index in 3" :key="index" class="recharge-order-card-skeleton">
            <span></span>
            <span></span>
            <span></span>
          </article>
        </div>
        <div v-else v-loading="loading" class="recharge-order-cards">
          <article v-for="row in orders" :key="row.id" class="recharge-order-card">
            <div>
              <span>订单号</span>
              <strong>{{ row.order_no }}</strong>
            </div>
            <div>
              <span>{{ t('time') }}</span>
              <strong>{{ formatTime(row.created_at) }}</strong>
            </div>
            <div>
              <span>{{ t('amount') }}</span>
              <strong>{{ formatMoney(row.amount_micros, locale, 2) }}</strong>
            </div>
            <div>
              <span>{{ t('payAmount') }}</span>
              <strong>{{ formatPayable(row) }}</strong>
            </div>
            <el-tag :type="orderStatusType(row.status)" effect="plain">
              {{ orderStatusLabel(row.status) }}
            </el-tag>
          </article>
          <el-empty v-if="!orders.length" :description="t('noData')" />
        </div>
      </div>
    </el-dialog>
  </section>
</template>

<style scoped>
.recharge-view {
  color: #354154;
  display: grid;
  font-family:
    Inter,
    ui-sans-serif,
    system-ui,
    -apple-system,
    BlinkMacSystemFont,
    'Segoe UI',
    'PingFang SC',
    'Microsoft YaHei',
    sans-serif;
  gap: 12px;
  width: min(1120px, 100%);
}

.recharge-layout {
  align-items: stretch;
  display: grid;
  gap: 18px;
  grid-template-columns: minmax(0, 1fr) minmax(280px, 330px);
}

.recharge-form-panel,
.recharge-summary-panel {
  display: grid;
  gap: 18px;
  padding: 20px;
}

.plan-card-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(auto-fit, minmax(168px, 1fr));
}

.plan-card {
  background: #ffffff;
  border: 1px solid #dfe7f1;
  border-radius: 8px;
  color: #354154;
  cursor: pointer;
  display: grid;
  gap: 8px;
  grid-template-rows: auto auto 1fr;
  min-height: 142px;
  padding: 15px;
  position: relative;
  text-align: left;
}

.plan-card.active {
  background: var(--user-primary-softer, #f6f9fc);
  border-color: var(--user-primary, #168bd3);
}

.plan-card-title {
  align-items: center;
  display: flex;
  gap: 8px;
  min-width: 0;
}

.plan-badge {
  background: var(--user-primary, #168bd3);
  border-radius: 999px;
  color: #fff;
  font-size: 12px;
  font-weight: 500;
  padding: 4px 9px;
  white-space: nowrap;
}

.plan-name {
  color: #354154;
  font-size: 14px;
  font-weight: 500;
  min-width: 0;
}

.plan-card strong {
  color: #1f2937;
  font-size: 25px;
  font-weight: 600;
  line-height: 1;
  white-space: nowrap;
}

.plan-hint {
  color: #7b8798;
  font-size: 12px;
  font-weight: 400;
}

.plan-card .el-icon {
  color: var(--user-primary, #168bd3);
  position: absolute;
  right: 16px;
  top: 16px;
}

.recharge-section {
  display: grid;
  gap: 12px;
}

.recharge-custom-section {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.recharge-section h3 {
  color: #667085;
  font-size: 13px;
  font-weight: 500;
  margin: 0;
}

.custom-amount-input {
  position: relative;
  width: min(70px, calc(100vw - 120px));
}

.custom-amount-input::before {
  color: #8a95a5;
  content: var(--recharge-currency-symbol);
  font-size: 14px;
  font-weight: 400;
  left: 10px;
  pointer-events: none;
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 1;
}

.custom-amount-input :deep(.el-input__inner) {
  font-size: 14px;
  font-weight: 400;
  height: 28px;
  padding-left: 18px;
  text-align: left;
}

.custom-amount-input :deep(.el-input__wrapper) {
  min-height: 28px;
  padding-inline: 4px;
}

.payment-methods {
  --el-segmented-bg-color: #ffffff;
  --el-segmented-item-selected-bg-color: var(--user-primary, #168bd3);
  border: 1px solid #dbe8f4;
  border-radius: 8px;
  padding: 2px;
  width: 100%;
}

.payment-methods :deep(.el-segmented__item) {
  border-radius: 6px;
  color: #b4bfcc;
  font-size: 13px;
  font-weight: 400;
  min-height: 42px;
  padding: 0 8px;
}

.payment-methods :deep(.el-segmented__item.is-selected) {
  box-shadow: 0 1px 3px rgb(22 139 211 / 18%);
  color: #ffffff;
}

.payment-methods :deep(.el-segmented__item:not(.is-selected):hover) {
  color: #b4bfcc;
}

.payment-methods :deep(.el-segmented__item-label) {
  align-items: center;
  display: flex;
  justify-content: center;
}

.payment-method-option {
  align-items: center;
  display: inline-flex;
  gap: 7px;
  justify-content: center;
  min-width: 0;
}

.payment-method-icon {
  flex: 0 0 auto;
  filter: grayscale(1) opacity(0.42);
  height: 22px;
  object-fit: contain;
  width: 22px;
}

.payment-methods :deep(.el-segmented__item.is-selected) .payment-method-icon {
  filter: brightness(0) invert(1);
}

.payment-methods :deep(.el-segmented__item:not(.is-selected):hover) .payment-method-icon {
  filter: grayscale(1) opacity(0.42);
}

.recharge-summary-panel {
  align-self: stretch;
  position: sticky;
  top: 20px;
}

.recharge-summary-total {
  display: grid;
  gap: 8px;
}

.recharge-summary-total span,
.recharge-summary-list dt {
  color: #667085;
  font-size: 13px;
  font-weight: 500;
}

.recharge-summary-total strong {
  color: #1f2937;
  font-size: 32px;
  font-weight: 600;
  line-height: 1.1;
}

.recharge-summary-list {
  border-bottom: 1px solid #edf1f6;
  border-top: 1px solid #edf1f6;
  display: grid;
  gap: 14px;
  margin: 0;
  padding: 16px 0;
}

.recharge-summary-list div {
  display: grid;
  gap: 8px;
}

.recharge-summary-list dd {
  color: #354154;
  font-size: 14px;
  font-weight: 400;
  margin: 0;
}

.recharge-submit {
  border-radius: 7px;
  font-size: 14px;
  font-weight: 500;
  min-height: 46px;
  width: 100%;
}

.recharge-history-trigger {
  color: #667085;
  font-size: 13px;
  font-weight: 400;
  padding-inline: 6px;
}

.recharge-dialog-toolbar {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 12px;
}

.recharge-order-loading {
  display: grid;
  gap: 10px;
  min-height: 220px;
}

.recharge-order-loading span {
  align-items: center;
  background: #fbfdff;
  border: 1px solid #edf1f6;
  border-radius: 8px;
  display: grid;
  gap: 16px;
  grid-template-columns: 1.4fr 0.9fr 0.7fr;
  min-height: 52px;
  padding: 0 16px;
}

.recharge-order-loading i,
.recharge-order-card-skeleton span {
  background: var(--skeleton-gradient);
  background-size: 220% 100%;
  border-radius: 999px;
  display: block;
  height: 12px;
}

.recharge-order-loading i:nth-child(2) {
  max-width: 120px;
}

.recharge-order-loading i:nth-child(3) {
  max-width: 86px;
}

.recharge-order-cards {
  display: none;
}

@media (max-width: 980px) {
  .recharge-layout {
    grid-template-columns: 1fr;
  }

  .plan-card-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .recharge-summary-panel {
    position: static;
  }
}

@media (max-width: 640px) {
  .plan-card-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .plan-card {
    min-height: 118px;
    padding: 12px;
  }

  .plan-card strong {
    font-size: 22px;
  }

  .plan-name {
    font-size: 13px;
  }

  .plan-hint {
    font-size: 12px;
    line-height: 1.35;
  }

  .recharge-form-panel,
  .recharge-summary-panel {
    padding: 18px;
  }

  .recharge-history-dialog-body .el-table {
    display: none;
  }

  .recharge-order-loading {
    display: none;
  }

  .recharge-order-cards {
    display: grid;
    gap: 10px;
    padding: 12px;
  }

  .recharge-order-card-loading {
    min-height: 200px;
  }

  .recharge-order-card-skeleton {
    background: #ffffff;
    border: 1px solid #edf1f6;
    border-radius: 8px;
    display: grid;
    gap: 10px;
    min-height: 90px;
    padding: 12px;
  }

  .recharge-order-card-skeleton span:nth-child(1) {
    width: 72%;
  }

  .recharge-order-card-skeleton span:nth-child(2) {
    width: 48%;
  }

  .recharge-order-card-skeleton span:nth-child(3) {
    width: 34%;
  }

  .recharge-order-card {
    background: #ffffff;
    border: 1px solid #edf1f6;
    border-radius: 8px;
    display: grid;
    gap: 10px;
    grid-template-columns: minmax(0, 1fr) auto;
    padding: 12px;
  }

  .recharge-order-card div {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .recharge-order-card div:first-child {
    grid-column: 1 / -1;
  }

  .recharge-order-card span {
    color: #8a95a5;
    font-size: 12px;
    font-weight: 400;
  }

  .recharge-order-card strong {
    color: #354154;
    font-size: 14px;
    font-weight: 400;
  }

  .recharge-order-card .el-tag {
    justify-self: end;
  }
}

@media (max-width: 420px) {
  .plan-card-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
