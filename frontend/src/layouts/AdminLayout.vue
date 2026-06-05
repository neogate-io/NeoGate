<script setup lang="ts">
import { computed } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'
import { Connection, Key, Monitor, Setting, SwitchButton, User } from '@element-plus/icons-vue'
import { isMessageKey } from '../i18n'
import { useLocale } from '../composables/useLocale'
import { useAuthStore } from '../stores/auth'

const auth = useAuthStore()
const { locale, t, toggleLocale } = useLocale()
const route = useRoute()
const router = useRouter()

const navItems = [
  { path: '/admin/channels', key: 'upstreamChannels', icon: Connection },
  { path: '/admin/credentials', key: 'credentialManagement', icon: Key },
  { path: '/admin/keys', key: 'userManagement', icon: User },
  { path: '/admin/usage', key: 'usage', icon: Monitor }
] as const

const settingItems = [
  { path: '/admin/settings/pricing-policies', key: 'pricingPolicy' }
] as const

const activeRoute = computed(() => route.path)
const settingsOpen = computed(() => route.path.startsWith('/admin/settings'))
const openMenus = computed(() => (settingsOpen.value ? ['settings'] : []))
const activeRouteLabel = computed(() => {
  const messageKey = route.meta.messageKey
  return t(isMessageKey(messageKey) ? messageKey : 'userManagement')
})
const nextLocaleLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中'))

async function logout() {
  auth.clearToken()
  await router.replace('/')
}
</script>

<template>
  <el-container class="app-shell admin-shell">
    <el-aside width="220px">
      <h1>{{ t('appName') }}</h1>
      <el-menu
        :key="settingsOpen ? 'settings-open' : 'settings-closed'"
        :default-active="activeRoute"
        :default-openeds="openMenus"
        mode="vertical"
        router
        unique-opened
      >
        <el-menu-item v-for="item in navItems" :key="item.path" :index="item.path">
          <el-icon><component :is="item.icon" /></el-icon>
          <span>{{ t(item.key) }}</span>
        </el-menu-item>
        <el-sub-menu index="settings">
          <template #title>
            <el-icon><Setting /></el-icon>
            <span>{{ t('settings') }}</span>
          </template>
          <el-menu-item v-for="item in settingItems" :key="item.path" :index="item.path">
            <span>{{ t(item.key) }}</span>
          </el-menu-item>
        </el-sub-menu>
      </el-menu>
    </el-aside>

    <el-main class="content">
      <header class="page-header">
        <h2>{{ activeRouteLabel }}</h2>
        <div class="header-actions">
          <el-button class="admin-action-button admin-language-button" :aria-label="t('language')" @click="toggleLocale">
            {{ nextLocaleLabel }}
          </el-button>
          <el-button class="admin-action-button" :icon="SwitchButton" @click="logout">{{ t('logout') }}</el-button>
        </div>
      </header>

      <RouterView />
    </el-main>
  </el-container>
</template>
