import { computed, reactive } from 'vue'
import { getPublicSiteSetting } from '../api/settings'

const DEFAULT_SITE_NAME = 'NeoGate'

const brand = reactive({
  loaded: false,
  loading: false,
  siteName: DEFAULT_SITE_NAME,
  logoUrl: '/logos/logo.svg'
})

let pendingLoad: Promise<void> | null = null

function applyBrand(setting: { site_name?: string | null; logo_url?: string | null }) {
  brand.siteName = setting.site_name?.trim() || DEFAULT_SITE_NAME
  brand.logoUrl = setting.logo_url?.trim() || ''
  brand.loaded = true
}

async function loadSiteBrand(force = false) {
  if (!force && brand.loaded) return
  if (!force && pendingLoad) return pendingLoad

  brand.loading = true
  pendingLoad = getPublicSiteSetting()
    .then(applyBrand)
    .catch(() => {
      brand.loaded = true
    })
    .finally(() => {
      brand.loading = false
      pendingLoad = null
    })

  return pendingLoad
}

export function setSiteBrand(setting: { site_name?: string | null; logo_url?: string | null }) {
  applyBrand(setting)
}

export function useSiteBrand() {
  void loadSiteBrand()

  return {
    siteName: computed(() => brand.siteName),
    logoUrl: computed(() => brand.logoUrl),
    loadSiteBrand,
    setSiteBrand
  }
}
