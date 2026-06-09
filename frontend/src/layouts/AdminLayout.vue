<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import {
  Connection,
  Key,
  Menu,
  Monitor,
  Setting,
  SwitchButton,
  User
} from '@element-plus/icons-vue'
import { getAdminServicePolicy } from '../api/policy'
import LocaleToggleButton from '../components/LocaleToggleButton.vue'
import { isMessageKey, type MessageKey } from '../i18n'
import { useLocale } from '../composables/useLocale'
import { useLogout } from '../composables/useLogout'
import { useAsyncData } from '../composables/useAsyncData'

const { t } = useLocale()
const route = useRoute()
const logout = useLogout(t)
const { data: servicePolicy } = useAsyncData(() => getAdminServicePolicy(), null)
const adminMenuOpen = ref(false)
type AdminNavItem = { path: string; key: MessageKey; icon: Component }
type SettingNavItem = { path: string; key: MessageKey }

const navItems: AdminNavItem[] = [
  { path: '/admin/channels', key: 'upstreamChannels', icon: Connection },
  { path: '/admin/credentials', key: 'credentialManagement', icon: Key },
  { path: '/admin/keys', key: 'userManagement', icon: User },
  { path: '/admin/usage', key: 'usage', icon: Monitor }
] as const

const settingItems = computed(() => {
  const items: SettingNavItem[] = [
    { path: '/admin/settings/admin-password', key: 'adminPasswordSettings' },
    { path: '/admin/settings/smtp', key: 'smtpSettings' }
  ]
  if (servicePolicy.value?.service_mode === 'paid') {
    items.push({ path: '/admin/settings/payment', key: 'paymentSettings' })
  }
  items.push({ path: '/admin/settings/pricing-policies', key: 'pricingPolicy' })
  items.push({ path: '/admin/settings/other', key: 'otherSettings' })
  return items
})

const activeRoute = computed(() => route.path)
const settingsOpen = computed(() => route.path.startsWith('/admin/settings'))
const openMenus = computed(() => (settingsOpen.value ? ['settings'] : []))
const activeRouteLabel = computed(() => {
  const messageKey = route.meta.messageKey
  return t(isMessageKey(messageKey) ? messageKey : 'userManagement')
})

watch(
  () => route.fullPath,
  () => {
    adminMenuOpen.value = false
  }
)
</script>

<template>
  <el-container class="app-shell light-sidebar-shell admin-shell">
    <el-aside :class="{ 'is-open': adminMenuOpen }" width="248px">
      <h1 class="shell-logo">
        <RouterLink class="shell-logo-link" to="/" :aria-label="t('home')">
          <img class="shell-logo-image" src="/logo.svg" :alt="t('appName')" />
        </RouterLink>
      </h1>
      <el-menu
        :key="settingsOpen ? 'settings-open' : 'settings-closed'"
        :default-active="activeRoute"
        :default-openeds="openMenus"
        mode="vertical"
        router
        unique-opened
        @select="adminMenuOpen = false"
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
    <button
      v-if="adminMenuOpen"
      class="admin-menu-overlay"
      type="button"
      :aria-label="t('adminMenu')"
      @click="adminMenuOpen = false"
    ></button>

    <el-main class="content">
      <header class="page-header">
        <el-button
          class="header-utility-button admin-menu-button"
          :aria-label="t('adminMenu')"
          :icon="Menu"
          @click="adminMenuOpen = true"
        />
        <h2>{{ activeRouteLabel }}</h2>
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
