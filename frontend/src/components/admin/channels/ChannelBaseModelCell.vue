<script setup lang="ts">
import { EditPen, Loading } from '@element-plus/icons-vue'
import type { InputInstance } from 'element-plus'
import { nextTick, ref } from 'vue'
import { useLocale } from '../../../composables/useLocale'

defineOptions({
  name: 'ChannelBaseModelCell'
})

const props = defineProps<{
  model: string
  baseModel: string | null
  saving: boolean
}>()

const emit = defineEmits<{
  save: [baseModel: string | null]
}>()

const { t } = useLocale()
const editing = ref(false)
const draft = ref('')
const inputRef = ref<InputInstance>()

function displayName() {
  const baseModel = props.baseModel?.trim()
  if (!baseModel || baseModel.toLowerCase() === props.model.trim().toLowerCase()) return '-'
  return baseModel
}

async function startEditing() {
  if (props.saving) return
  editing.value = true
  draft.value = props.baseModel ?? ''
  await nextTick()
  inputRef.value?.focus()
  inputRef.value?.select()
}

function cancelEditing() {
  editing.value = false
  draft.value = ''
}

function save() {
  if (!editing.value) return
  const baseModel = draft.value.trim() || null
  const currentBaseModel = props.baseModel?.trim() || null
  cancelEditing()
  if (baseModel !== currentBaseModel) emit('save', baseModel)
}
</script>

<template>
  <span class="channel-base-model" :class="{ 'is-editing': editing }">
    <el-input
      v-if="editing"
      ref="inputRef"
      v-model="draft"
      :aria-label="t('baseModel')"
      :placeholder="t('baseModel')"
      maxlength="255"
      size="small"
      @click.stop
      @blur="save"
      @keyup.enter="save"
      @keydown.esc.stop.prevent="cancelEditing"
    />
    <template v-else>
      <span class="channel-base-model-text">{{ displayName() }}</span>
      <el-tooltip :content="t('editBaseModel')" placement="top" :show-after="600">
        <el-button
          class="channel-base-model-edit-button"
          text
          :disabled="saving"
          :class="{ 'is-loading': saving }"
          :aria-label="`${t('editBaseModel')}: ${model}`"
          @click.stop="startEditing"
        >
          <el-icon :class="{ 'is-loading': saving }">
            <Loading v-if="saving" />
            <EditPen v-else />
          </el-icon>
        </el-button>
      </el-tooltip>
    </template>
  </span>
</template>

<style scoped>
.channel-base-model {
  align-items: center;
  color: #4e5969;
  display: flex;
  font-size: 12px;
  gap: 6px;
  min-height: 30px;
  min-width: 0;
  overflow: hidden;
  padding: 0;
  width: 100%;
}

.channel-base-model-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-base-model-edit-button.el-button {
  align-items: center;
  color: #64748b;
  display: inline-flex;
  flex: 0 0 22px;
  height: 22px;
  justify-content: center;
  margin: 0;
  min-height: 22px;
  opacity: 0;
  padding: 0;
  width: 22px;
}

.channel-base-model-edit-button.el-button .el-icon {
  font-size: 13px;
}

.channel-base-model:hover .channel-base-model-edit-button.el-button,
.channel-base-model-edit-button.el-button:focus-visible,
.channel-base-model-edit-button.el-button.is-loading {
  opacity: 1;
}

.channel-base-model-edit-button.el-button:hover,
.channel-base-model-edit-button.el-button:focus-visible {
  background: #eef6ff;
  color: var(--admin-primary);
  outline: none;
}

.channel-base-model.is-editing {
  overflow: visible;
}

.channel-base-model.is-editing :deep(.el-input) {
  width: 160px;
}

.channel-base-model.is-editing :deep(.el-input__wrapper) {
  box-shadow: 0 0 0 1px var(--admin-primary) inset;
}
</style>
