<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Check, Clock, Warning } from '@element-plus/icons-vue'
import { getRechargeOrders, type PaymentOrder } from '../../api/recharge'
import { useLocale } from '../../composables/useLocale'
import { isAbortError } from '../../utils/async'
import { formatDateTime } from '../../utils/format'
import { paymentReturnState, pollPaymentOrder } from '../../utils/payment'
import '../../styles/user.css'

const route = useRoute()
const router = useRouter()
const { locale, t } = useLocale()
const order = ref<PaymentOrder | null>(null)
const checking = ref(false)
const checked = ref(false)
const checkFailed = ref(false)
let checkGeneration = 0
let checkController: AbortController | null = null

const MAX_CHECK_ATTEMPTS = 12
const CHECK_INTERVAL_MS = 1500

const queryValue = (keys: string[]) => {
  for (const key of keys) {
    const value = route.query[key]
    if (Array.isArray(value)) {
      const first = value.find(Boolean)
      if (first) return first
    } else if (value) {
      return value
    }
  }
  return ''
}

const returnedOrderNo = computed(() =>
  queryValue(['out_trade_no', 'order_no', 'orderNo', 'order_id'])
)
const orderNo = computed(() => String(order.value?.order_no ?? (returnedOrderNo.value || '-')))

const viewState = computed<'checking' | 'paid' | 'pending' | 'failed' | 'unknown' | 'error'>(() => {
  if (checking.value) return 'checking'
  if (checkFailed.value) return 'error'
  return paymentReturnState(order.value)
})

const viewMessages = {
  checking: ['paymentReturnChecking', 'paymentReturnCheckingHint'],
  paid: ['paymentReturnPaid', 'paymentReturnPaidHint'],
  pending: ['paymentReturnPending', 'paymentReturnPendingHint'],
  failed: ['paymentReturnFailed', 'paymentReturnFailedHint'],
  error: ['paymentReturnError', 'paymentReturnErrorHint'],
  unknown: ['paymentReturnUnknown', 'paymentReturnUnknownHint']
} as const
const title = computed(() => t(viewMessages[viewState.value][0]))
const hint = computed(() => t(viewMessages[viewState.value][1]))

const payAmount = computed(() => {
  if (!order.value) return '-'
  const amount = order.value.payable_amount_minor / 100
  return `${order.value.currency} ${amount.toLocaleString(locale.value, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  })}`
})

const payTime = computed(() => {
  const value = order.value?.paid_at ?? order.value?.updated_at
  return value ? formatDateTime(value, locale.value) : '-'
})

async function verifyPayment() {
  checkController?.abort()
  const controller = new AbortController()
  checkController = controller
  const expectedOrderNo = returnedOrderNo.value
  const generation = ++checkGeneration
  order.value = null
  checked.value = false
  checkFailed.value = false

  if (!expectedOrderNo) {
    checking.value = false
    checked.value = true
    checkController = null
    return
  }

  checking.value = true
  try {
    const result = await pollPaymentOrder(
      expectedOrderNo,
      (signal) => getRechargeOrders({ signal }),
      {
        attempts: MAX_CHECK_ATTEMPTS,
        intervalMs: CHECK_INTERVAL_MS,
        signal: controller.signal
      }
    )
    if (generation === checkGeneration) order.value = result
  } catch (error) {
    if (isAbortError(error)) return
    if (generation === checkGeneration) checkFailed.value = true
  } finally {
    if (generation === checkGeneration) {
      checking.value = false
      checked.value = true
      checkController = null
    }
  }
}

function returnToUserHome() {
  router.push({ name: 'userOverview' })
}

onMounted(() => void verifyPayment())
onBeforeUnmount(() => {
  checkGeneration += 1
  checkController?.abort()
  checkController = null
})
</script>

<template>
  <main class="payment-return-shell">
    <section class="payment-return-panel user-panel">
      <div class="payment-return-icon" :class="viewState">
        <el-icon v-if="viewState === 'paid'"><Check /></el-icon>
        <el-icon v-else-if="['failed', 'unknown', 'error'].includes(viewState)"
          ><Warning
        /></el-icon>
        <el-icon v-else><Clock /></el-icon>
      </div>
      <h1>{{ title }}</h1>
      <p class="payment-return-hint">{{ hint }}</p>
      <dl class="payment-return-details">
        <div>
          <dt>{{ t('paymentOrderNo') }}</dt>
          <dd>{{ orderNo }}</dd>
        </div>
        <div>
          <dt>{{ t('paymentAmount') }}</dt>
          <dd>{{ payAmount }}</dd>
        </div>
        <div>
          <dt>{{ t('paymentTime') }}</dt>
          <dd>{{ payTime }}</dd>
        </div>
      </dl>
      <div class="payment-return-actions">
        <el-button
          v-if="checked && viewState !== 'paid'"
          size="large"
          :loading="checking"
          @click="verifyPayment"
        >
          {{ t('paymentCheckAgain') }}
        </el-button>
        <el-button type="primary" size="large" @click="returnToUserHome">
          {{ t('returnToUserHome') }}
        </el-button>
      </div>
    </section>
  </main>
</template>

<style scoped>
.payment-return-shell {
  display: grid;
  place-items: center;
  min-height: calc(100vh - 96px);
  padding: 24px;
}

.payment-return-panel {
  display: grid;
  gap: 16px;
  justify-items: center;
  max-width: 520px;
  padding: 28px;
  text-align: center;
}

.payment-return-icon {
  align-items: center;
  display: inline-flex;
  height: 56px;
  justify-content: center;
  width: 56px;
  border-radius: 14px;
  background: #eef4fb;
  color: #52708f;
  font-size: 28px;
}

.payment-return-icon.paid {
  background: rgba(41, 157, 92, 0.12);
  color: #299d5c;
}

.payment-return-icon.failed,
.payment-return-icon.unknown,
.payment-return-icon.error {
  background: #fef3f2;
  color: #b42318;
}

.payment-return-hint {
  color: var(--text-secondary);
  line-height: 1.6;
  margin: -6px 0 4px;
  max-width: 420px;
}

.payment-return-panel h1 {
  margin: 0;
  font-size: 24px;
  line-height: 1.2;
}

.payment-return-details {
  display: grid;
  gap: 12px;
  margin: 0;
  width: 100%;
}

.payment-return-details div {
  align-items: start;
  display: grid;
  gap: 10px;
  grid-template-columns: 88px minmax(0, 1fr);
  text-align: left;
}

.payment-return-details dt {
  margin: 0;
  color: var(--text-secondary);
  font-size: 14px;
}

.payment-return-details dd {
  color: #111827;
  font-weight: 700;
  margin: 0;
  min-width: 0;
  overflow-wrap: anywhere;
}

.payment-return-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  justify-content: center;
  width: 100%;
}

.payment-return-actions .el-button {
  min-width: 168px;
}

@media (max-width: 520px) {
  .payment-return-panel {
    padding: 24px 20px;
    width: 100%;
  }

  .payment-return-details div {
    grid-template-columns: 76px minmax(0, 1fr);
  }

  .payment-return-actions .el-button {
    width: 100%;
  }
}
</style>
