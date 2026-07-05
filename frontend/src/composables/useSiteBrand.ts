import { computed, reactive } from 'vue'
import { getPublicSiteSetting, type SiteSetting } from '../api/settings'

const DEFAULT_SITE_NAME = 'NeoGate'
const DEFAULT_BILLING_CURRENCY = 'CNY' as const

type BillingCurrency = 'USD' | 'CNY'

const brand = reactive({
  loaded: false,
  loading: false,
  siteName: DEFAULT_SITE_NAME,
  logoUrl: '/logos/logo.svg',
  billingCurrency: DEFAULT_BILLING_CURRENCY as BillingCurrency
})

let pendingLoad: Promise<void> | null = null

function normalizeBillingCurrency(value?: string | null): BillingCurrency {
  return value === 'CNY' ? 'CNY' : 'USD'
}

function applyBrand(setting: SiteSetting) {
  brand.siteName = setting.site_name?.trim() || DEFAULT_SITE_NAME
  brand.logoUrl = setting.logo_url?.trim() || ''
  brand.billingCurrency = normalizeBillingCurrency(setting.billing_currency)
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

export function setSiteBrand(setting: SiteSetting) {
  applyBrand(setting)
}

export function useSiteBrand() {
  void loadSiteBrand()

  return {
    siteName: computed(() => brand.siteName),
    logoUrl: computed(() => brand.logoUrl),
    billingCurrency: computed(() => brand.billingCurrency),
    loadSiteBrand,
    setSiteBrand
  }
}
