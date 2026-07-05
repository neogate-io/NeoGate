import type { PricingTemplate, ProviderPrice } from '../types/admin'

const CONFIRMED_PRICE_SOURCE = 'confirmed_price'

export function priceKey(provider: string, model: string) {
  return `${provider.trim()}\u0000${model.trim().toLowerCase()}`
}

export function derivedCacheReadPrice(inputPrice: number) {
  return Math.round(inputPrice / 10)
}

export function isProviderPriceConfigured(price?: ProviderPrice) {
  if (!price) return false
  if (price.billing_meter === 'image') {
    return price.unit_price_micros !== undefined && price.unit_price_micros !== null
  }
  return price.input_price_micros >= 0 && price.output_price_micros >= 0
}

export function isProviderPriceReady(price?: ProviderPrice) {
  return Boolean(price?.enabled && isProviderPriceConfigured(price))
}

export function findPricingTemplate(templates: PricingTemplate[], provider: string, model: string) {
  const normalizedProvider = provider.trim()
  const normalizedModel = model.trim().toLowerCase()
  const enabledTemplates = templates.filter(
    (template) =>
      template.enabled &&
      template.source !== CONFIRMED_PRICE_SOURCE &&
      template.model.trim().toLowerCase() === normalizedModel
  )
  const exact = enabledTemplates.find((template) => template.provider.trim() === normalizedProvider)

  if (exact) {
    return exact
  }
  return enabledTemplates.find((template) => template.provider.trim() !== normalizedProvider)
}
