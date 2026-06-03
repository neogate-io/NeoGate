<script setup lang="ts">
import { computed, watchEffect } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import en from 'element-plus/es/locale/lang/en'
import { isMessageKey } from './i18n'
import { useLocale } from './composables/useLocale'

const route = useRoute()
const { locale, t } = useLocale()
const elementLocale = computed(() => (locale.value === 'zh-CN' ? zhCn : en))

watchEffect(() => {
  if (typeof document === 'undefined') return

  const messageKey = route.meta.messageKey
  const appTitle = t('appTitle')
  const pageTitle = isMessageKey(messageKey) ? t(messageKey) : appTitle

  document.title = messageKey === 'home' ? t('tagline') : `${pageTitle} - ${appTitle}`
  document.documentElement.lang = locale.value
})
</script>

<template>
  <el-config-provider :locale="elementLocale">
    <RouterView />
  </el-config-provider>
</template>
