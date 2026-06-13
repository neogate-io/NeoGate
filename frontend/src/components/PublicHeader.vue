<script setup lang="ts">
import { computed } from 'vue'
import { UserFilled } from '@element-plus/icons-vue'
import { RouterLink } from 'vue-router'
import LocaleToggleButton from './LocaleToggleButton.vue'
import { useLocale } from '../composables/useLocale'
import { useAuthStore } from '../stores/auth'

defineProps<{
  headerClass?: string
}>()

const GITHUB_URL = 'https://github.com/neogate-io/NeoGate'
const auth = useAuthStore()
const { t } = useLocale()
const dashboardLink = computed(() => (auth.isAdmin ? '/admin' : '/home/overview'))
</script>

<template>
  <header :class="['home-header', headerClass]">
    <RouterLink class="home-brand" to="/" :aria-label="t('appName')">
      <img class="home-brand-logo" src="/logo.svg" :alt="t('appName')" />
    </RouterLink>
    <nav class="home-nav" :aria-label="t('appName')">
      <RouterLink to="/">{{ t('home') }}</RouterLink>
      <RouterLink to="/docs">{{ t('docs') }}</RouterLink>
      <RouterLink to="/interfaces">{{ t('interfaces') }}</RouterLink>
    </nav>
    <div class="home-header-actions">
      <LocaleToggleButton class="home-language-button" />
      <a
        class="github-link"
        :href="GITHUB_URL"
        target="_blank"
        rel="noreferrer"
        :aria-label="t('github')"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M12 2C6.48 2 2 6.58 2 12.26c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49 0-.24-.01-.88-.01-1.73-2.78.62-3.37-1.38-3.37-1.38-.45-1.19-1.11-1.5-1.11-1.5-.91-.64.07-.63.07-.63 1 .07 1.53 1.06 1.53 1.06.9 1.57 2.35 1.12 2.93.86.09-.67.35-1.12.63-1.38-2.22-.26-4.56-1.14-4.56-5.07 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.33 9.33 0 0 1 12 6.98c.85 0 1.7.12 2.5.34 1.9-1.33 2.74-1.05 2.74-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.94-2.34 4.81-4.57 5.07.36.32.68.95.68 1.92 0 1.38-.01 2.49-.01 2.83 0 .27.18.59.69.49A10.15 10.15 0 0 0 22 12.26C22 6.58 17.52 2 12 2Z"
          />
        </svg>
      </a>
      <RouterLink
        v-if="auth.isAuthed"
        class="home-login-link home-account-link"
        :to="dashboardLink"
        :aria-label="t('admin')"
      >
        <el-icon><UserFilled /></el-icon>
      </RouterLink>
      <RouterLink v-else class="home-login-link" to="/login">{{ t('signIn') }}</RouterLink>
    </div>
  </header>
</template>
