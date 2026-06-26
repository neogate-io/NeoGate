<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import {
  Connection,
  FolderOpened,
  Key,
  Menu,
  Monitor,
  Promotion,
  Setting,
  SwitchButton,
  User
} from '@element-plus/icons-vue'
import { getAdminServicePolicy } from '../api/policy'
import LocaleToggleButton from '../components/common/LocaleToggleButton.vue'
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
type AdminNavGroup = { key: MessageKey; items: AdminNavItem[] }

const navGroups = computed(() => {
  const operationItems: AdminNavItem[] = [
    { path: '/admin/channels', key: 'upstreamChannels', icon: Connection }
  ]
  if (servicePolicy.value?.service_mode === 'internal') {
    operationItems.push({ path: '/admin/apps', key: 'apps', icon: Promotion })
  }
  if (servicePolicy.value?.service_mode !== 'internal') {
    operationItems.push({ path: '/admin/credentials', key: 'credentialManagement', icon: Key })
    operationItems.push({ path: '/admin/keys', key: 'userManagement', icon: User })
  }
  operationItems.push({ path: '/admin/usage', key: 'usage', icon: Monitor })
  operationItems.push({ path: '/admin/statistics', key: 'usageStatistics', icon: Monitor })
  const groups: AdminNavGroup[] = [
    {
      key: 'adminNavOperations',
      items: operationItems
    }
  ]
  if (servicePolicy.value?.service_mode !== 'paid') {
    groups.push({
      key: 'adminNavAccounts',
      items: [
        { path: '/admin/keys', key: 'userManagement', icon: User },
        { path: '/admin/projects', key: 'projectManagement', icon: FolderOpened }
      ]
    })
  }
  return groups
})

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
const activeRouteSubtitle = computed(() => {
  const subtitleKey = route.meta.subtitleKey
  return t(isMessageKey(subtitleKey) ? subtitleKey : 'adminConsoleSubtitle')
})
const activeRouteGroupLabel = computed(() => {
  if (settingsOpen.value) return t('settings')
  const matchedGroup = navGroups.value.find((group) =>
    group.items.some((item) => activeRoute.value.startsWith(item.path))
  )
  return t(matchedGroup?.key ?? 'adminNavOperations')
})
const activeBreadcrumbs = computed(() => [
  t('admin'),
  activeRouteGroupLabel.value,
  activeRouteLabel.value
])

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
          <img class="shell-logo-image" src="/logos/logo.svg" :alt="t('appName')" />
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
        <template v-for="group in navGroups" :key="group.key">
          <li class="admin-nav-group-label">{{ t(group.key) }}</li>
          <el-menu-item v-for="item in group.items" :key="item.path" :index="item.path">
            <el-icon><component :is="item.icon" /></el-icon>
            <span>{{ t(item.key) }}</span>
          </el-menu-item>
        </template>
        <li class="admin-nav-group-label">{{ t('adminNavSystem') }}</li>
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
        <div class="page-topbar">
          <div class="page-topbar-start">
            <el-button
              class="header-utility-button admin-menu-button"
              :aria-label="t('adminMenu')"
              :icon="Menu"
              @click="adminMenuOpen = true"
            />
            <nav class="page-breadcrumb" :aria-label="t('admin')">
              <template v-for="(breadcrumb, index) in activeBreadcrumbs" :key="breadcrumb">
                <span>{{ breadcrumb }}</span>
                <span v-if="index < activeBreadcrumbs.length - 1" aria-hidden="true">/</span>
              </template>
            </nav>
          </div>
          <div class="header-actions">
            <el-tooltip :content="t('language')" placement="bottom" :show-after="600">
              <LocaleToggleButton class="header-utility-button header-language-button" />
            </el-tooltip>
            <span class="header-action-divider" aria-hidden="true"></span>
            <el-tooltip :content="t('logout')" placement="bottom" :show-after="600">
              <el-button
                class="header-utility-button header-logout-button"
                :aria-label="t('logout')"
                :icon="SwitchButton"
                @click="logout"
              />
            </el-tooltip>
          </div>
        </div>

        <div class="page-title-block">
          <h2>{{ activeRouteLabel }}</h2>
          <span>{{ activeRouteSubtitle }}</span>
        </div>
      </header>

      <RouterView />
    </el-main>
  </el-container>
</template>
