<script setup lang="ts">
import { Connection } from '@element-plus/icons-vue'
import { computed, ref, watch } from 'vue'

const props = defineProps<{
  provider: string
}>()

const iconUrl: Record<string, string> = {
  newapi: '/icons/newapi.png',
  openai: '/icons/openai.svg',
  anthropic: '/icons/anthropic.ico',
  google: '/icons/gemini.png',
  gemini: '/icons/gemini.png',
  deepseek: '/icons/deepseek.ico',
  qwen: '/icons/qwen.svg',
  moonshot: '/icons/kimi.ico',
  kimi: '/icons/kimi.ico',
  zhipu: '/icons/glm.ico',
  glm: '/icons/glm.ico',
  doubao: '/icons/doubao.png',
  baidu: '/icons/baidu.png',
  tencent: '/icons/tencent.ico',
  minimax: '/icons/minimax.ico',
  stepfun: '/icons/stepfun.ico',
  baichuan: '/icons/baichuan.png',
  iflytek: '/icons/iflytek.ico',
  sensenova: '/icons/sensenova.ico',
  siliconflow: '/icons/siliconflow.ico',
  jdcloud: '/icons/jdcloud.png'
}

const imageFailed = ref(false)
const providerKey = computed(() => props.provider.trim().toLowerCase())
const imageSrc = computed(() => iconUrl[providerKey.value] ?? '')
const hasImage = computed(() => Boolean(imageSrc.value) && !imageFailed.value)

watch(imageSrc, () => {
  imageFailed.value = false
})

function handleError() {
  imageFailed.value = true
}
</script>

<template>
  <span
    class="provider-icon"
    :class="{ 'has-symbol': !hasImage, 'has-image': hasImage }"
    aria-hidden="true"
  >
    <img v-if="hasImage" :src="imageSrc" alt="" @error="handleError" />
    <Connection v-else class="provider-icon-symbol" />
  </span>
</template>

<style scoped>
.provider-icon {
  align-items: center;
  background: #f1f5f9;
  border: 1px solid #d7dee8;
  border-radius: 6px;
  color: #334155;
  display: inline-flex;
  flex: 0 0 auto;
  font-size: 10px;
  font-weight: 820;
  height: 22px;
  justify-content: center;
  line-height: 1;
  min-width: 22px;
  padding: 0 4px;
}

.provider-icon.has-image {
  background: transparent;
  border-color: transparent;
  padding: 0;
  width: 24px;
}

.provider-icon.has-symbol {
  background: transparent;
  border-color: transparent;
  color: #475569;
  padding: 0;
}

.provider-icon-symbol {
  height: 14px;
  width: 14px;
}

.provider-icon.has-image img {
  border-radius: 5px;
  display: block;
  height: 18px;
  object-fit: contain;
  width: 18px;
}

</style>
