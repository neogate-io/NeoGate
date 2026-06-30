<script setup lang="ts">
import { Right } from '@element-plus/icons-vue'
import { useLocale } from '../../../composables/useLocale'
import { formatMicroUsd } from '../../../utils/format'

const open = defineModel<boolean>('open', { required: true })
const amount = defineModel<number>('amount', { required: true })

defineProps<{
  title: string
  currentBalanceMicroUsd: number
  adjustedBalanceMicroUsd: number
  hint: string
  confirmText: string
  saving: boolean
  subjectLabel?: string
  subjectName?: string | null
}>()

const emit = defineEmits<{
  submit: []
}>()

const { t } = useLocale()
</script>

<template>
  <el-dialog
    v-model="open"
    class="user-admin-dialog user-credit-dialog credit-adjust-dialog"
    :title="title"
    width="380px"
  >
    <div class="credit-adjust-dialog-body">
      <div v-if="subjectName" class="credit-adjust-subject">
        <span>{{ subjectLabel }}</span>
        <strong>{{ subjectName }}</strong>
      </div>

      <section class="credit-adjust-balance-card" aria-live="polite">
        <div class="credit-adjust-balance-item">
          <span>{{ t('currentBalance') }}</span>
          <strong>{{ formatMicroUsd(currentBalanceMicroUsd, 2) }}</strong>
        </div>
        <div class="credit-adjust-balance-arrow" aria-hidden="true">
          <el-icon><Right /></el-icon>
        </div>
        <div class="credit-adjust-balance-item is-after">
          <span>{{ t('afterAdjustment') }}</span>
          <strong>{{ formatMicroUsd(adjustedBalanceMicroUsd, 2) }}</strong>
        </div>
      </section>

      <div class="credit-adjust-amount-section">
        <label class="credit-adjust-amount-label">{{ t('amountUsd') }}</label>
        <el-input-number
          v-model="amount"
          :controls="false"
          :min="-100000"
          :precision="0"
          :step="1"
        />
        <p class="credit-adjust-hint">{{ hint }}</p>
      </div>
    </div>

    <template #footer>
      <div class="admin-dialog-footer user-dialog-footer">
        <el-button @click="open = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="emit('submit')">
          {{ confirmText }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
:global(.credit-adjust-dialog .el-dialog__body) {
  padding-top: 8px;
}

:global(.credit-adjust-dialog .el-dialog__footer) {
  padding-top: 8px;
}

:global(.credit-adjust-dialog .admin-dialog-footer) {
  border-top: 0;
}

.credit-adjust-dialog-body {
  display: grid;
  gap: 16px;
}

.credit-adjust-subject {
  align-items: baseline;
  display: flex;
  gap: 8px;
  min-width: 0;
}

.credit-adjust-subject span {
  color: #667085;
  flex: 0 0 auto;
  font-size: 12.5px;
  font-weight: 600;
  line-height: 1.2;
}

.credit-adjust-subject strong {
  color: #1d2939;
  font-size: 14px;
  font-weight: 650;
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.credit-adjust-balance-card {
  align-items: center;
  background: #f8fbff;
  border: 1px solid #e5eef8;
  border-radius: 8px;
  display: grid;
  gap: 10px;
  grid-template-columns: minmax(0, 1fr) 28px minmax(0, 1fr);
  padding: 12px 14px;
}

.credit-adjust-balance-item {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.credit-adjust-balance-item span {
  color: #667085;
  font-size: 12.5px;
  font-weight: 600;
  line-height: 1.2;
}

.credit-adjust-balance-item strong {
  color: #1d2939;
  font-feature-settings: 'tnum';
  font-size: 17px;
  font-variant-numeric: tabular-nums;
  font-weight: 700;
  line-height: 1.25;
}

.credit-adjust-balance-arrow {
  align-items: center;
  background: #ffffff;
  border: 1px solid #d7e5f3;
  border-radius: 999px;
  color: #7a8aa0;
  display: inline-flex;
  height: 28px;
  justify-content: center;
  width: 28px;
}

.credit-adjust-balance-item.is-after {
  align-items: end;
  text-align: right;
}

.credit-adjust-balance-item.is-after strong {
  color: var(--admin-primary);
}

.credit-adjust-amount-section {
  align-items: center;
  display: grid;
  gap: 8px 14px;
  grid-template-columns: minmax(0, 1fr) 150px;
}

.credit-adjust-amount-label {
  color: #3f4a5c;
  font-size: 13px;
  font-weight: 650;
  line-height: 1.2;
}

.credit-adjust-amount-section :deep(.el-input-number) {
  justify-self: end;
  width: 100%;
}

.credit-adjust-amount-section :deep(.el-input__wrapper) {
  border-radius: 7px;
  min-height: 40px;
}

.credit-adjust-amount-section :deep(.el-input__inner) {
  font-feature-settings: 'tnum';
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.credit-adjust-hint {
  color: #7a8798;
  font-size: 12px;
  grid-column: 1 / -1;
  line-height: 1.5;
  margin: 0;
}

@media (max-width: 720px) {
  .credit-adjust-balance-card {
    grid-template-columns: 1fr;
  }

  .credit-adjust-balance-arrow {
    justify-self: start;
    transform: rotate(90deg);
  }

  .credit-adjust-balance-item.is-after {
    align-items: start;
    text-align: left;
  }

  .credit-adjust-amount-section {
    grid-template-columns: 1fr;
  }

  .credit-adjust-amount-section :deep(.el-input-number) {
    justify-self: stretch;
  }
}
</style>
