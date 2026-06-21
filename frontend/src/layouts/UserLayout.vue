<script setup lang="ts">
import { computed, provide, type Component } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import { DataBoard, Key, Monitor, Setting, SwitchButton, Wallet } from '@element-plus/icons-vue'
import { getUserServicePolicy } from '../api/policy'
import LocaleToggleButton from '../components/common/LocaleToggleButton.vue'
import { isMessageKey, type MessageKey } from '../i18n'
import { useLocale } from '../composables/useLocale'
import { useLogout } from '../composables/useLogout'
import { useAsyncData } from '../composables/useAsyncData'

const { t } = useLocale()
const route = useRoute()
const logout = useLogout(t)
const { data: servicePolicy } = useAsyncData(() => getUserServicePolicy(), null)
provide('servicePolicy', servicePolicy)
type NavItem = { path: string; key: MessageKey; icon: Component }

const navItems = computed(() => {
  const items: NavItem[] = [
    { path: '/home/overview', key: 'overview', icon: DataBoard },
    { path: '/home/apikeys', key: 'apiKey', icon: Key },
    { path: '/home/usage', key: 'usage', icon: Monitor }
  ]
  if (servicePolicy.value?.recharge_enabled) {
    items.push({ path: '/home/recharge', key: 'recharge', icon: Wallet })
  }
  items.push({ path: '/home/settings', key: 'personalSettings', icon: Setting })
  return items
})

const activeRoute = computed(() => route.path)
const activeRouteLabel = computed(() => {
  const messageKey = route.meta.messageKey
  return t(isMessageKey(messageKey) ? messageKey : 'apiKey')
})
const activeRouteSubtitle = computed(() => {
  const subtitleKey = route.meta.subtitleKey
  return t(isMessageKey(subtitleKey) ? subtitleKey : 'userConsoleStatus')
})
</script>

<template>
  <el-container class="app-shell light-sidebar-shell user-shell">
    <el-aside width="248px">
      <h1 class="shell-logo">
        <RouterLink class="shell-logo-link" to="/" :aria-label="t('home')">
          <img class="shell-logo-image" src="/logos/logo.svg" :alt="t('appName')" />
        </RouterLink>
      </h1>
      <el-menu :default-active="activeRoute" mode="vertical" router>
        <el-menu-item v-for="item in navItems" :key="item.path" :index="item.path">
          <el-icon><component :is="item.icon" /></el-icon>
          <span>{{ t(item.key) }}</span>
        </el-menu-item>
      </el-menu>
    </el-aside>

    <el-main class="content">
      <header class="page-header">
        <div class="page-title-block">
          <h2>{{ activeRouteLabel }}</h2>
          <span>{{ activeRouteSubtitle }}</span>
        </div>
        <div class="header-actions">
          <el-tooltip :content="t('language')" placement="bottom">
            <LocaleToggleButton class="header-utility-button header-language-button" />
          </el-tooltip>
          <span class="header-action-divider" aria-hidden="true"></span>
          <el-tooltip :content="t('logout')" placement="bottom">
            <el-button
              class="header-utility-button header-logout-button"
              :aria-label="t('logout')"
              :icon="SwitchButton"
              @click="logout"
            />
          </el-tooltip>
        </div>
      </header>

      <RouterView />
    </el-main>
  </el-container>
</template>
