<script setup lang="ts">
import { computed, watchEffect } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import en from 'element-plus/es/locale/lang/en'
import { isMessageKey } from './i18n'
import { useLocale } from './composables/useLocale'
import { useSiteBrand } from './composables/useSiteBrand'

const route = useRoute()
const { locale, t } = useLocale()
const { siteName } = useSiteBrand()
const elementLocale = computed(() => (locale.value === 'zh-CN' ? zhCn : en))
const showPoweredFooter = computed(
  () => !route.matched.some((record) => record.meta.admin === true || record.meta.user === true)
)
const rootShellClass = computed(() => {
  const routeName = typeof route.name === 'string' ? route.name : ''

  return {
    'root-shell-without-footer': !showPoweredFooter.value,
    [`root-shell-route-${routeName}`]: routeName
  }
})

watchEffect(() => {
  if (typeof document === 'undefined') return

  const messageKey = route.meta.messageKey
  const appTitle = siteName.value || t('appTitle')
  const pageTitle = isMessageKey(messageKey) ? t(messageKey) : appTitle

  document.title =
    messageKey === 'home' ? t('tagline', { siteName: appTitle }) : `${pageTitle} - ${appTitle}`
  document.documentElement.lang = locale.value
})
</script>

<template>
  <el-config-provider :locale="elementLocale">
    <div class="root-shell" :class="rootShellClass">
      <div class="root-view">
        <RouterView />
      </div>
      <footer v-if="showPoweredFooter" class="powered-footer">
        <a href="https://github.com/neogate-io/NeoGate" target="_blank" rel="noopener noreferrer">
          Powered by NeoGate
        </a>
      </footer>
    </div>
  </el-config-provider>
</template>
