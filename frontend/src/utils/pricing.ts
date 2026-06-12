import type { PricingTemplate, ProviderPrice } from '../types/admin'

const CONFIRMED_PRICE_SOURCE = 'confirmed_price'
const MANUAL_BASE_URL_PROVIDERS = new Set(['custom', 'newapi'])

export function priceKey(provider: string, model: string) {
  return `${provider}\u0000${model}`
}

export function derivedCacheReadPrice(inputPrice: number) {
  return Math.round(inputPrice / 10)
}

export function isProviderPriceConfigured(price?: ProviderPrice) {
  return Boolean(price && price.input_price_usd_micros > 0 && price.output_price_usd_micros > 0)
}

export function isProviderPriceReady(price?: ProviderPrice) {
  return Boolean(price?.enabled && isProviderPriceConfigured(price))
}

export function findPricingTemplate(templates: PricingTemplate[], provider: string, model: string) {
  const normalizedProvider = provider.trim()
  const normalizedModel = model.trim()
  const enabledTemplates = templates.filter(
    (template) => template.enabled && template.model === normalizedModel
  )
  const exact = enabledTemplates.find((template) => template.provider === normalizedProvider)

  if (exact && exact.source !== CONFIRMED_PRICE_SOURCE) {
    return exact
  }
  if (!MANUAL_BASE_URL_PROVIDERS.has(normalizedProvider)) {
    return undefined
  }

  return findUniqueExternalTemplate(enabledTemplates, normalizedProvider)
}

function findUniqueExternalTemplate(templates: PricingTemplate[], provider: string) {
  const candidates = templates.filter(
    (template) => template.provider !== provider && template.source !== CONFIRMED_PRICE_SOURCE
  )
  return candidates.length === 1 ? candidates[0] : undefined
}
