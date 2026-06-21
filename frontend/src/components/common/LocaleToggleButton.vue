<script setup lang="ts">
import { computed } from 'vue'
import { useLocale } from '../../composables/useLocale'

const { locale, t, toggleLocale } = useLocale()
const nextLocaleLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中'))
</script>

<template>
  <el-button :aria-label="t('language')" @click="toggleLocale">
    {{ nextLocaleLabel }}
  </el-button>
</template>
