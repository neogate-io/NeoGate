<script setup lang="ts">
import { computed } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'
import { DataBoard, Key, Monitor, SwitchButton, Wallet } from '@element-plus/icons-vue'
import { ElMessageBox } from 'element-plus'
import { isMessageKey } from '../i18n'
import { useLocale } from '../composables/useLocale'
import { useAuthStore } from '../stores/auth'

const auth = useAuthStore()
const { locale, t, toggleLocale } = useLocale()
const route = useRoute()
const router = useRouter()

const navItems = [
  { path: '/home/overview', key: 'overview', icon: DataBoard },
  { path: '/home/apikeys', key: 'apiKey', icon: Key },
  { path: '/home/usage', key: 'usage', icon: Monitor },
  { path: '/home/recharge', key: 'recharge', icon: Wallet }
] as const

const activeRoute = computed(() => route.path)
const activeRouteLabel = computed(() => {
  const messageKey = route.meta.messageKey
  return t(isMessageKey(messageKey) ? messageKey : 'apiKey')
})
const activeRouteSubtitle = computed(() => {
  const subtitleKey = route.meta.subtitleKey
  return t(isMessageKey(subtitleKey) ? subtitleKey : 'userConsoleStatus')
})
const nextLocaleLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中'))

async function logout() {
  try {
    await ElMessageBox.confirm(t('logoutConfirmMessage'), t('logoutConfirmTitle'), {
      confirmButtonText: t('logout'),
      cancelButtonText: t('cancel'),
      type: 'warning'
    })
  } catch {
    return
  }

  auth.clearToken()
  await router.replace('/')
}
</script>

<template>
  <el-container class="app-shell user-shell">
    <el-aside width="248px">
      <h1 class="shell-logo">
        <RouterLink class="shell-logo-link" to="/" :aria-label="t('home')">
          <img class="shell-logo-image" src="/logo.svg" :alt="t('appName')" />
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
            <el-button class="header-utility-button header-language-button" :aria-label="t('language')" @click="toggleLocale">
              {{ nextLocaleLabel }}
            </el-button>
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
