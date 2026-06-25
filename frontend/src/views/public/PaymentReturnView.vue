<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowRight, Check, Wallet } from '@element-plus/icons-vue'
import { useAuthStore } from '../../stores/auth'
import { useLocale } from '../../composables/useLocale'

const { t } = useLocale()
const router = useRouter()
const auth = useAuthStore()
const redirecting = ref(false)

onMounted(async () => {
  if (!auth.isAuthed) return

  redirecting.value = true
  const ok = await auth.verifySession()
  if (ok) {
    await router.replace('/home/recharge')
  }
  redirecting.value = false
})
</script>

<template>
  <main class="payment-return-shell">
    <section class="payment-return-panel user-panel">
      <div class="payment-return-icon">
        <el-icon><Check /></el-icon>
      </div>
      <h1>{{ t('paymentSettings') }}</h1>
      <p>支付已经完成。余额会在后台回调确认后更新。</p>
      <p v-if="redirecting">正在返回充值页。</p>
      <div class="payment-return-actions">
        <el-button :icon="Wallet" type="primary" @click="router.push('/home/recharge')">
          前往充值页
        </el-button>
        <el-button :icon="ArrowRight" @click="router.push('/login')">前往登录</el-button>
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

.payment-return-panel p {
  margin: 0;
  color: var(--text-secondary);
}

.payment-return-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  justify-content: center;
}
</style>
