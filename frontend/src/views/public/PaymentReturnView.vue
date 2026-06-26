<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Check } from '@element-plus/icons-vue'
import { useLocale } from '../../composables/useLocale'
import { formatDateTime } from '../../utils/format'

const route = useRoute()
const router = useRouter()
const { locale } = useLocale()

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

const orderNo = computed(
  () => queryValue(['out_trade_no', 'order_no', 'orderNo', 'order_id', 'trade_no']) || '-'
)

const payAmount = computed(() => {
  const raw = queryValue(['money', 'amount', 'pay_amount', 'payAmount'])
  const amount = Number(raw)
  if (!Number.isFinite(amount) || amount <= 0) return '-'
  return `¥${amount.toLocaleString(locale.value, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  })}`
})

const payTime = computed(() => {
  const raw = queryValue(['pay_time', 'payTime', 'endtime', 'timestamp', 'time'])
  const numeric = Number(raw)
  const value =
    Number.isFinite(numeric) && numeric > 0
      ? new Date(numeric < 10_000_000_000 ? numeric * 1000 : numeric).toISOString()
      : raw || new Date().toISOString()
  return formatDateTime(value, locale.value)
})

function returnToUserHome() {
  router.push({ name: 'userOverview' })
}
</script>

<template>
  <main class="payment-return-shell">
    <section class="payment-return-panel user-panel">
      <div class="payment-return-icon">
        <el-icon><Check /></el-icon>
      </div>
      <h1>支付成功</h1>
      <dl class="payment-return-details">
        <div>
          <dt>订单号</dt>
          <dd>{{ orderNo }}</dd>
        </div>
        <div>
          <dt>支付金额</dt>
          <dd>{{ payAmount }}</dd>
        </div>
        <div>
          <dt>支付时间</dt>
          <dd>{{ payTime }}</dd>
        </div>
      </dl>
      <div class="payment-return-actions">
        <el-button type="primary" size="large" @click="returnToUserHome">返回用户后台</el-button>
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
  background: rgba(41, 157, 92, 0.12);
  color: #299d5c;
  font-size: 28px;
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
