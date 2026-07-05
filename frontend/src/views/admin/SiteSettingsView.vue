<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Link as LinkIcon, Monitor, Refresh } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getAdminServicePolicy } from '../../api/policy'
import { checkLatestVersion, getSiteSetting, saveSiteSetting } from '../../api/settings'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import { setSiteBrand } from '../../composables/useSiteBrand'
import type { VersionCheckResult } from '../../types/admin'
import { readError } from '../../utils/errors'
import { formatDateTime } from '../../utils/format'

const { locale, t } = useLocale()

const loading = ref(false)
const siteSettingSaving = ref(false)
const siteForm = ref({
  siteName: '',
  logoUrl: '',
  publicBaseUrl: '',
  envWriteSupported: false
})
const checkingVersion = ref(false)
const versionCheck = ref<VersionCheckResult | null>(null)

const siteSettingDescription = computed(() => {
  return siteForm.value.envWriteSupported
    ? t('siteSettingsDescription')
    : t('siteSettingsReadOnlyDescription')
})
const versionStatusLabel = computed(() => {
  if (!versionCheck.value) return t('versionNotChecked')
  return versionCheck.value.update_available ? t('versionUpdateAvailable') : t('versionUpToDate')
})
const versionStatusType = computed(() => {
  if (!versionCheck.value) return 'info'
  return versionCheck.value.update_available ? 'warning' : 'success'
})
const versionPublishedAt = computed(() => {
  return formatDateTime(versionCheck.value?.published_at, locale.value)
})

function applySiteSetting(setting: Awaited<ReturnType<typeof getSiteSetting>>) {
  siteForm.value = {
    siteName: setting.site_name || 'NeoGate',
    logoUrl: setting.logo_url ?? '',
    publicBaseUrl: setting.public_base_url ?? '',
    envWriteSupported: setting.env_write_supported
  }
}

async function load() {
  await withLoading(loading, async () => {
    try {
      applySiteSetting(await getSiteSetting())
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function saveSiteConfig() {
  const siteName = siteForm.value.siteName.trim()
  const logoUrl = siteForm.value.logoUrl.trim()
  const publicBaseUrl = siteForm.value.publicBaseUrl.trim()
  if (!siteName) {
    ElMessage.error(t('siteNameRequired'))
    return
  }
  if (!publicBaseUrl) {
    ElMessage.error(t('publicBaseUrlRequired'))
    return
  }
  try {
    const url = new URL(publicBaseUrl)
    if (!['http:', 'https:'].includes(url.protocol)) throw new Error('invalid protocol')
  } catch {
    ElMessage.error(t('publicBaseUrlInvalid'))
    return
  }
  if (logoUrl) {
    try {
      const url = new URL(logoUrl)
      if (!['http:', 'https:'].includes(url.protocol)) throw new Error('invalid protocol')
    } catch {
      ElMessage.error(t('siteLogoUrlInvalid'))
      return
    }
  }

  await withLoading(siteSettingSaving, async () => {
    try {
      const result = await saveSiteSetting({
        site_name: siteName,
        public_base_url: publicBaseUrl,
        logo_url: logoUrl || null
      })
      applySiteSetting(result.setting)
      setSiteBrand(result.setting)
      await getAdminServicePolicy(true).catch(() => null)
      ElMessage.success(
        result.restart_required ? t('siteSettingsSavedRestartRequired') : t('siteSettingsSaved')
      )
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

async function checkVersion() {
  await withLoading(checkingVersion, async () => {
    try {
      versionCheck.value = await checkLatestVersion()
      ElMessage.success(versionStatusLabel.value)
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
}

onMounted(load)
</script>

<template>
  <section class="admin-settings-view site-settings-view">
    <div v-loading="loading" class="site-settings-grid">
      <section class="site-settings-card">
        <header class="admin-settings-section-header site-settings-card-header">
          <el-icon class="admin-settings-panel-icon"><Monitor /></el-icon>
          <div class="site-settings-card-copy">
            <h3>{{ t('siteSettings') }}</h3>
            <p>{{ siteSettingDescription }}</p>
          </div>
        </header>
        <el-form
          class="site-settings-inline-form"
          label-position="top"
          @submit.prevent="saveSiteConfig"
        >
          <el-form-item :label="t('siteNameLabel')">
            <el-input
              v-model="siteForm.siteName"
              :disabled="siteSettingSaving"
              :placeholder="t('siteNamePlaceholder')"
            />
          </el-form-item>
          <el-form-item :label="t('siteLogoUrlLabel')">
            <el-input
              v-model="siteForm.logoUrl"
              :disabled="siteSettingSaving"
              :placeholder="t('siteLogoUrlPlaceholder')"
            />
          </el-form-item>
          <el-form-item :label="t('publicBaseUrlLabel')">
            <el-input
              v-model="siteForm.publicBaseUrl"
              :disabled="!siteForm.envWriteSupported || siteSettingSaving"
              :placeholder="t('publicBaseUrlPlaceholder')"
            />
          </el-form-item>
        </el-form>
        <div class="site-settings-actions">
          <el-button
            class="admin-action-button"
            type="primary"
            :loading="siteSettingSaving"
            @click="saveSiteConfig"
          >
            {{ t('save') }}
          </el-button>
        </div>
      </section>

      <section class="site-settings-card">
        <header class="admin-settings-section-header site-settings-card-header">
          <el-icon class="admin-settings-panel-icon"><Refresh /></el-icon>
          <div class="site-settings-card-copy">
            <div class="version-heading-row">
              <h3>{{ t('versionCheck') }}</h3>
              <el-tag class="version-status-tag" :type="versionStatusType" effect="light" round>
                {{ versionStatusLabel }}
              </el-tag>
            </div>
            <p>{{ t('versionCheckDescription') }}</p>
            <p class="site-settings-meta">
              <span>{{ t('currentVersion') }}</span>
              <strong>{{ versionCheck?.current_version ?? '-' }}</strong>
              <span>{{ t('latestVersion') }}</span>
              <strong>{{ versionCheck?.latest_tag ?? '-' }}</strong>
            </p>
            <p v-if="versionCheck" class="site-settings-meta">
              <span>{{ t('releasePublishedAt') }}</span>
              <strong>{{ versionPublishedAt }}</strong>
            </p>
          </div>
        </header>
        <div class="site-settings-actions">
          <el-button
            v-if="versionCheck"
            class="admin-action-button"
            :icon="LinkIcon"
            tag="a"
            :href="versionCheck.release_url"
            target="_blank"
            rel="noopener noreferrer"
          >
            {{ t('viewRelease') }}
          </el-button>
          <el-button
            class="admin-action-button"
            type="primary"
            :icon="Refresh"
            :loading="checkingVersion"
            @click="checkVersion"
          >
            {{ t('checkLatestVersion') }}
          </el-button>
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
:global(.site-settings-view.admin-settings-view) {
  align-items: flex-start;
}

.site-settings-grid {
  display: grid;
  gap: 16px;
  width: min(780px, 100%);
}

.site-settings-card {
  background: var(--admin-surface);
  border: 1px solid var(--admin-border);
  border-radius: var(--admin-radius);
  box-shadow: none;
  display: grid;
  gap: 16px;
  min-width: 0;
  padding: 20px 22px;
}

.site-settings-card-header {
  align-items: flex-start;
  grid-template-columns: 28px minmax(0, 1fr) auto;
}

.site-settings-card-copy {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.site-settings-card-copy h3 {
  color: var(--admin-text);
  font-size: 14px;
  font-weight: 760;
  line-height: 1.25;
  margin: 0;
}

.version-heading-row {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.version-status-tag.el-tag {
  animation: none;
  transition: none;
}

.version-status-tag.el-tag :deep(*) {
  animation: none;
  transition: none;
}

.site-settings-card-copy p {
  color: var(--admin-text-muted);
  font-size: 13px;
  font-weight: 560;
  line-height: 1.6;
  margin: 0;
}

.site-settings-meta {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 4px;
}

.site-settings-meta strong {
  color: var(--admin-text);
  font-weight: 720;
}

.site-settings-inline-form {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.3fr);
  margin-left: 38px;
}

.site-settings-inline-form :deep(.el-form-item) {
  margin-bottom: 0;
}

.site-settings-inline-form :deep(.el-form-item__label) {
  color: var(--admin-text-muted);
  font-size: 12px;
  font-weight: 700;
  line-height: 1.3;
  margin-bottom: 6px;
}

.site-settings-actions {
  border-top: 1px solid var(--admin-border-soft);
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  justify-content: flex-end;
  padding-top: 16px;
}

@media (max-width: 640px) {
  .site-settings-card {
    padding: 18px 16px;
  }

  .site-settings-card-header {
    grid-template-columns: 28px minmax(0, 1fr);
  }

  .site-settings-actions {
    justify-content: stretch;
  }

  .site-settings-actions .el-button {
    flex: 1 1 0;
    min-width: 0;
  }

  .site-settings-inline-form {
    grid-template-columns: minmax(0, 1fr);
    margin-left: 0;
  }
}
</style>
