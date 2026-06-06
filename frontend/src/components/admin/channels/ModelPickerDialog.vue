<script setup lang="ts">
import { useLocale } from '../../../composables/useLocale'

const open = defineModel<boolean>('open', { required: true })
const selectedModels = defineModel<string[]>('selectedModels', { required: true })

defineProps<{
  models: string[]
  allSelected: boolean
}>()

const emit = defineEmits<{
  toggleAll: [checked: boolean]
}>()

const { t } = useLocale()
</script>

<template>
  <el-dialog
    v-model="open"
    class="model-picker-dialog"
    :title="t('selectModels')"
    width="560px"
    append-to-body
  >
    <div class="model-picker">
      <div class="model-picker-toolbar">
        <span class="model-count">
          {{ t('selectedModelCount') }} {{ selectedModels.length }}/{{ models.length }}
        </span>
      </div>

      <div class="model-select-panel">
        <div class="model-checkbox-list">
          <label class="model-checkbox-item model-checkbox-all">
            <input
              type="checkbox"
              :checked="allSelected"
              @change="emit('toggleAll', ($event.target as HTMLInputElement).checked)"
            />
            <span>{{ t('allModels') }}</span>
          </label>
          <label v-for="model in models" :key="model" class="model-checkbox-item">
            <input v-model="selectedModels" type="checkbox" :value="model" />
            <span>{{ model }}</span>
          </label>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="dialog-footer">
        <el-button type="primary" @click="open = false">
          {{ t('save') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.model-picker {
  display: grid;
  gap: 10px;
}

.model-picker-toolbar {
  align-items: center;
  display: flex;
  justify-content: flex-start;
}

.model-count {
  color: #667085;
  font-size: 13px;
  font-weight: 700;
  white-space: nowrap;
}

.model-select-panel {
  display: grid;
  gap: 8px;
}

.model-checkbox-list {
  align-content: start;
  background: #ffffff;
  border: 1px solid #e3e8ef;
  border-radius: 8px;
  display: grid;
  gap: 0;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  max-height: 270px;
  overflow: auto;
  padding: 6px;
}

.model-checkbox-item {
  align-items: center;
  border-radius: 6px;
  color: #475569;
  cursor: pointer;
  display: grid;
  gap: 9px;
  grid-template-columns: 16px minmax(0, 1fr);
  height: 32px;
  min-width: 0;
  padding: 0 7px;
}

.model-checkbox-item:hover {
  background: #f8fafc;
}

.model-checkbox-all {
  border-bottom: 1px solid #edf1f6;
  color: #1f2937;
  font-weight: 760;
  grid-column: 1 / -1;
  margin-bottom: 3px;
}

.model-checkbox-item input {
  accent-color: var(--brand-blue);
  cursor: pointer;
  height: 16px;
  margin: 0;
  width: 16px;
}

.model-checkbox-item span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dialog-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

:global(.model-picker-dialog) {
  border-radius: 10px;
  max-width: calc(100vw - 32px);
}

:global(.model-picker-dialog .el-dialog__header) {
  margin: 0;
  padding: 22px 24px 10px;
}

:global(.model-picker-dialog .el-dialog__title) {
  color: #111827;
  font-size: 21px;
  font-weight: 800;
  line-height: 1.2;
}

:global(.model-picker-dialog .el-dialog__headerbtn) {
  right: 18px;
  top: 14px;
}

:global(.model-picker-dialog .el-dialog__body) {
  padding: 14px 24px 8px;
}

:global(.model-picker-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 24px 20px;
}

.dialog-footer :deep(.el-button) {
  border-radius: 8px;
  font-weight: 740;
  min-height: 40px;
  min-width: 86px;
}

@media (max-width: 760px) {
  .model-picker-toolbar {
    align-items: stretch;
    display: grid;
    grid-template-columns: 1fr;
  }

  .model-count {
    white-space: normal;
  }

  .model-checkbox-list {
    grid-template-columns: 1fr;
  }
}
</style>
