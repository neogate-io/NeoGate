<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import { splitCommaList } from '../../../utils/channel'

const open = defineModel<boolean>('open', { required: true })
const selectedModels = defineModel<string[]>('selectedModels', { required: true })
const models = defineModel<string[]>('models', { required: true })

const props = defineProps<{
  allSelected: boolean
}>()

const emit = defineEmits<{
  toggleAll: [checked: boolean]
}>()

const { t } = useLocale()
const manualModelInput = ref('')
const modelSearchQuery = ref('')

const filteredModels = computed(() => {
  const query = modelSearchQuery.value.trim().toLowerCase()
  if (!query) return models.value
  return models.value.filter((model) => model.toLowerCase().includes(query))
})

watch(open, (isOpen) => {
  if (!isOpen) {
    modelSearchQuery.value = ''
  }
})

function addManualModels() {
  const manualModels = splitCommaList(manualModelInput.value)
  if (manualModels.length === 0) return

  const existing = new Set(models.value)
  const nextModels = [...models.value]
  for (const model of manualModels) {
    if (existing.has(model)) continue
    existing.add(model)
    nextModels.push(model)
  }

  models.value = nextModels
  selectedModels.value = Array.from(new Set([...selectedModels.value, ...manualModels]))
  manualModelInput.value = ''
}
</script>

<template>
  <el-dialog
    v-model="open"
    class="model-picker-dialog"
    :close-on-click-modal="false"
    :title="t('selectModels')"
    width="560px"
    append-to-body
  >
    <div class="model-picker">
      <div class="model-picker-toolbar">
        <el-input
          v-model="modelSearchQuery"
          class="model-search-input"
          clearable
          :placeholder="t('modelSearchPlaceholder')"
        />
        <span class="model-count">
          {{ t('selectedModelCount') }} {{ selectedModels.length }}/{{ models.length }}
        </span>
      </div>

      <div class="model-select-panel">
        <div class="model-checkbox-list">
          <label class="model-checkbox-item model-checkbox-all">
            <input
              type="checkbox"
              :checked="props.allSelected"
              @change="emit('toggleAll', ($event.target as HTMLInputElement).checked)"
            />
            <span>{{ t('allModels') }}</span>
          </label>
          <label v-for="model in filteredModels" :key="model" class="model-checkbox-item">
            <input v-model="selectedModels" type="checkbox" :value="model" />
            <span>{{ model }}</span>
          </label>
          <div v-if="filteredModels.length === 0" class="model-empty-state">
            {{ t('noMatchingModels') }}
          </div>
        </div>
        <div class="manual-model-add">
          <el-input
            v-model="manualModelInput"
            class="manual-model-input"
            :placeholder="t('manualModelPlaceholder')"
            @keyup.enter="addManualModels"
          />
          <el-button @click="addManualModels">{{ t('addManualModel') }}</el-button>
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
  gap: 12px;
  justify-content: space-between;
  min-width: 0;
}

.model-count {
  color: #667085;
  font-size: 13px;
  font-weight: 700;
  white-space: nowrap;
}

.model-search-input {
  flex: 0 1 240px;
  min-width: 180px;
}

.model-search-input :deep(.el-input__wrapper) {
  border-radius: 7px;
  min-height: 32px;
}

.manual-model-add {
  align-items: center;
  display: flex;
  gap: 8px;
  justify-content: stretch;
  min-width: 0;
  width: 100%;
}

.manual-model-input {
  flex: 1;
  min-width: 0;
}

.manual-model-add :deep(.el-button) {
  border-radius: 7px;
  flex: 0 0 auto;
  font-weight: 680;
  min-height: 32px;
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
  color: #1f2937;
  font-weight: 760;
  grid-column: 1 / -1;
}

.model-empty-state {
  align-items: center;
  color: #94a3b8;
  display: flex;
  font-size: 13px;
  grid-column: 1 / -1;
  min-height: 56px;
  padding: 0 8px;
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
  border-radius: 8px;
  max-width: calc(100vw - 32px);
}

:global(.model-picker-dialog .el-dialog__header) {
  margin: 0;
  padding: 18px 22px 14px;
}

:global(.model-picker-dialog .el-dialog__title) {
  color: #111827;
  font-size: 18px;
  font-weight: 760;
  line-height: 1.2;
}

:global(.model-picker-dialog .el-dialog__body) {
  padding: 18px 22px;
}

:global(.model-picker-dialog .el-dialog__footer) {
  border-top: 1px solid #edf1f6;
  padding: 14px 22px 18px;
}

.dialog-footer :deep(.el-button) {
  border-radius: 7px;
  font-weight: 680;
  min-height: 34px;
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

  .model-search-input {
    min-width: 0;
    width: 100%;
  }

  .manual-model-add {
    display: grid;
    grid-template-columns: 1fr;
    justify-content: stretch;
  }

  .manual-model-input {
    max-width: none;
  }

  .model-checkbox-list {
    grid-template-columns: 1fr;
  }
}
</style>
