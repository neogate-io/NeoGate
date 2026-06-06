<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Briefcase, CreditCard, Check } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import {
  completeSetup,
  getSetupStatus,
  type ServiceMode,
  type ServicePolicy
} from '../../api/policy'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { readError } from '../../utils/errors'

const router = useRouter()
const { t } = useLocale()
const loading = ref(false)
const saving = ref(false)
const policy = ref<ServicePolicy | null>(null)
const selectedMode = ref<ServiceMode>('internal')

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

async function load() {
  loading.value = true
  try {
    policy.value = await getSetupStatus()
    if (policy.value.setup_completed) {
      await router.replace('/login')
    }
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function submit() {
  saving.value = true
  try {
    await completeSetup(selectedMode.value)
    ElMessage.success(t('setupCompleted'))
    await router.replace('/login')
  } catch (err) {
    ElMessage.error(readError(err))
    await load()
  } finally {
    saving.value = false
  }
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

      <div class="setup-mode-grid" role="radiogroup" :aria-label="t('serviceMode')">
        <button
          v-for="item in modeOptions"
          :key="item.value"
          class="setup-mode-card"
          :class="{ active: selectedMode === item.value }"
          type="button"
          role="radio"
          :aria-checked="selectedMode === item.value"
          @click="selectedMode = item.value"
        >
          <span class="setup-mode-icon">
            <el-icon><component :is="item.icon" /></el-icon>
          </span>
          <span class="setup-mode-copy">
            <strong>{{ item.title }}</strong>
            <span>{{ item.description }}</span>
          </span>
          <el-icon v-if="selectedMode === item.value" class="setup-mode-check"><Check /></el-icon>
        </button>
      </div>

      <el-button
        class="setup-submit"
        type="primary"
        size="large"
        :loading="saving"
        @click="submit"
      >
        {{ t('completeSetup') }}
      </el-button>
    </section>
  </main>
</template>

<style scoped>
.setup-shell {
  align-items: center;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.92), rgba(246, 249, 252, 0.94)),
    #f6f9fc;
  display: grid;
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
  gap: 22px;
  justify-self: center;
  width: min(760px, 100%);
}

.setup-heading {
  display: grid;
  gap: 10px;
  text-align: center;
}

.setup-heading h1 {
  color: #111827;
  font-size: clamp(30px, 5vw, 44px);
  font-weight: 840;
  letter-spacing: 0;
  line-height: 1.08;
  margin: 0;
}

.setup-heading p {
  color: #697586;
  font-size: 16px;
  font-weight: 560;
  line-height: 1.7;
  margin: 0 auto;
  max-width: 620px;
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
  box-shadow: 0 14px 42px rgba(15, 23, 42, 0.07);
  color: #111827;
  cursor: pointer;
  display: grid;
  gap: 16px;
  min-height: 210px;
  padding: 20px;
  position: relative;
  text-align: left;
}

.setup-mode-card.active {
  border-color: #168bd3;
  box-shadow: 0 18px 52px rgba(22, 139, 211, 0.18);
}

.setup-mode-icon {
  align-items: center;
  background: #eef6fb;
  border-radius: 8px;
  color: #168bd3;
  display: inline-flex;
  height: 42px;
  justify-content: center;
  width: 42px;
}

.setup-mode-icon .el-icon {
  font-size: 22px;
}

.setup-mode-copy {
  display: grid;
  gap: 8px;
}

.setup-mode-copy strong {
  color: #111827;
  font-size: 18px;
  font-weight: 820;
}

.setup-mode-copy span {
  color: #64748b;
  font-size: 14px;
  font-weight: 560;
  line-height: 1.6;
}

.setup-mode-check {
  color: #168bd3;
  font-size: 20px;
  position: absolute;
  right: 18px;
  top: 18px;
}

.setup-submit {
  border-radius: 7px;
  font-weight: 780;
  justify-self: center;
  min-width: 180px;
}

@media (max-width: 700px) {
  .setup-shell {
    align-items: start;
    padding: 76px 16px 24px;
  }

  .setup-mode-grid {
    grid-template-columns: 1fr;
  }

  .setup-mode-card {
    min-height: 168px;
  }
}
</style>
