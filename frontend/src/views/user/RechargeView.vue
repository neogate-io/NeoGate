<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { Check, CreditCard, Refresh, View } from '@element-plus/icons-vue'
import { createRechargeOrder, getRechargeOrders, type PaymentOrder, type PayType } from '../../api/recharge'
import { useLocale } from '../../composables/useLocale'
import { readError } from '../../utils/errors'

const { locale, t } = useLocale()
const usdPerCny = 5
const selectedAmount = ref(100)
const customAmount = ref<number | null>(null)
const payType = ref<PayType>('wxpay')
const submitting = ref(false)
const historyDialogVisible = ref(false)
const orders = ref<PaymentOrder[]>([])
const ordersLoaded = ref(false)
const loading = ref(false)

const plans = computed(() => [
  { key: 'trial', amount: 10, name: t('trialPlan'), hint: t('trialPlanHint') },
  { key: 'builder', amount: 50, name: t('builderPlan'), hint: t('builderPlanHint') },
  { key: 'growth', amount: 100, name: t('growthPlan'), hint: t('growthPlanHint'), recommended: true },
  { key: 'pro', amount: 200, name: t('proPlan'), hint: t('proPlanHint') },
  { key: 'business', amount: 1000, name: t('businessPlan'), hint: t('businessPlanHint') },
  { key: 'enterprise', amount: 2000, name: t('enterprisePlan'), hint: t('enterprisePlanHint') }
])

const amountUsd = computed(() => {
  const custom = Number(customAmount.value)
  return Number.isInteger(custom) && custom > 0 ? custom : selectedAmount.value
})

const amountMicroUsd = computed(() => Math.round(amountUsd.value * 1_000_000))
const payableCny = computed(() => amountUsd.value / usdPerCny)

function selectPlan(amount: number) {
  selectedAmount.value = amount
  customAmount.value = null
}

function formatUsd(microUsd: number) {
  return `$${(microUsd / 1_000_000).toFixed(2)}`
}

function formatUsdAmount(amount: number) {
  return `$${amount.toLocaleString(locale.value, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 0
  })}`
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
  return new Date(value).toLocaleString(locale.value)
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

async function openRechargeHistory() {
  historyDialogVisible.value = true
  if (!ordersLoaded.value) await reloadOrders()
}

async function reloadOrders() {
  loading.value = true
  try {
    orders.value = await getRechargeOrders()
    ordersLoaded.value = true
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function submitRecharge() {
  if (!Number.isFinite(amountUsd.value) || !Number.isInteger(amountUsd.value) || amountUsd.value <= 0) {
    ElMessage.error(t('rechargeAmountRequired'))
    return
  }

  submitting.value = true
  try {
    const result = await createRechargeOrder(amountMicroUsd.value, payType.value, window.location.href)
    if (ordersLoaded.value || historyDialogVisible.value) await reloadOrders()
    if (result.checkout_url) {
      window.location.href = result.checkout_url
    } else {
      ElMessage.success(t('rechargeOrderCreated'))
    }
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    submitting.value = false
  }
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
            :class="{ active: customAmount == null && selectedAmount === plan.amount, recommended: plan.recommended }"
            type="button"
            @click="selectPlan(plan.amount)"
          >
            <span v-if="plan.recommended" class="plan-badge">{{ t('recommended') }}</span>
            <span class="plan-name">{{ plan.name }}</span>
            <strong>{{ formatUsdAmount(plan.amount) }}</strong>
            <span class="plan-hint">{{ plan.hint }}</span>
            <el-icon v-if="customAmount == null && selectedAmount === plan.amount"><Check /></el-icon>
          </button>
        </div>

        <div class="recharge-section recharge-custom-section">
          <h3>{{ t('customAmount') }}</h3>
          <el-input-number
            v-model="customAmount"
            class="custom-amount-input"
            :min="1"
            :max="1000000"
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
          <el-button class="recharge-history-trigger" text :icon="View" :loading="loading" @click="openRechargeHistory">
            {{ t('viewRechargeOrders') }}
          </el-button>
        </div>
        <div class="recharge-summary-total">
          <span>{{ t('payAmount') }}</span>
          <strong>{{ formatCny(payableCny) }}</strong>
        </div>
        <dl class="recharge-summary-list">
          <div>
            <dt>{{ t('creditedAmount') }}</dt>
            <dd>{{ formatUsdAmount(amountUsd) }}</dd>
          </div>
          <div>
            <dt>{{ t('exchangeRate') }}</dt>
            <dd>{{ t('oneUsdEqualsCny') }}</dd>
          </div>
          <div>
            <dt>{{ t('paymentMethod') }}</dt>
            <dd>
              <el-segmented
                v-model="payType"
                class="payment-methods"
                :options="[
                  { label: t('wechatPay'), value: 'wxpay' },
                  { label: t('alipay'), value: 'alipay' }
                ]"
              />
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
        <el-table v-loading="loading" class="admin-table service-table" :data="orders" stripe>
          <el-table-column :label="t('time')" min-width="170">
            <template #default="{ row }">{{ formatTime(row.created_at) }}</template>
          </el-table-column>
          <el-table-column :label="t('amount')" min-width="110">
            <template #default="{ row }">{{ formatUsd(row.amount_micro_usd) }}</template>
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
        <div v-loading="loading" class="recharge-order-cards">
          <article v-for="row in orders" :key="row.id" class="recharge-order-card">
            <div>
              <span>{{ t('time') }}</span>
              <strong>{{ formatTime(row.created_at) }}</strong>
            </div>
            <div>
              <span>{{ t('amount') }}</span>
              <strong>{{ formatUsd(row.amount_micro_usd) }}</strong>
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
  display: grid;
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
  color: #111827;
  cursor: pointer;
  display: grid;
  gap: 8px;
  min-height: 142px;
  padding: 15px;
  position: relative;
  text-align: left;
}

.plan-card.active {
  background: var(--user-primary-softer, #f6f9fc);
  border-color: var(--user-primary, #168bd3);
}

.plan-badge {
  background: var(--user-primary, #168bd3);
  border-radius: 999px;
  color: #fff;
  font-size: 12px;
  font-weight: 780;
  justify-self: start;
  padding: 4px 9px;
}

.plan-name {
  color: #334155;
  font-size: 14px;
  font-weight: 800;
}

.plan-card strong {
  color: #111827;
  font-size: 26px;
  font-weight: 840;
  line-height: 1;
  white-space: nowrap;
}

.plan-hint {
  color: #8a95a5;
  font-size: 12px;
  font-weight: 560;
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
  color: #697586;
  font-size: 13px;
  font-weight: 700;
  margin: 0;
}

.custom-amount-input {
  position: relative;
  width: min(104px, calc(100vw - 120px));
}

.custom-amount-input::before {
  color: #8a95a5;
  content: "$";
  font-size: 14px;
  font-weight: 720;
  left: 12px;
  pointer-events: none;
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 1;
}

.custom-amount-input :deep(.el-input__inner) {
  padding-left: 26px;
}

.payment-methods {
  width: 100%;
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
  color: #697586;
  font-size: 13px;
  font-weight: 700;
}

.recharge-summary-total strong {
  color: #111827;
  font-size: 34px;
  font-weight: 820;
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
  color: #111827;
  font-size: 14px;
  font-weight: 740;
  margin: 0;
}

.recharge-submit {
  border-radius: 7px;
  font-weight: 760;
  width: 100%;
}

.recharge-history-trigger {
  color: #697586;
  font-weight: 720;
  padding-inline: 6px;
}

.recharge-dialog-toolbar {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 12px;
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

  .recharge-order-cards {
    display: grid;
    gap: 10px;
    padding: 12px;
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
    font-weight: 720;
  }

  .recharge-order-card strong {
    color: #111827;
    font-size: 14px;
    font-weight: 740;
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
