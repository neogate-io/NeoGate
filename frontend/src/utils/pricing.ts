import type { PricingTemplate } from '../types/admin'

const CONFIRMED_PRICE_SOURCE = 'confirmed_price'
const CUSTOM_PROVIDER = 'custom'

export function priceKey(provider: string, model: string) {
  return `${provider}\u0000${model}`
}

export function findPricingTemplate(
  templates: PricingTemplate[],
  provider: string,
  model: string
) {
  const normalizedProvider = provider.trim()
  const normalizedModel = model.trim()
  const enabledTemplates = templates.filter(
    (template) => template.enabled && template.model === normalizedModel
  )
  const exact = enabledTemplates.find((template) => template.provider === normalizedProvider)

  if (exact && exact.source !== CONFIRMED_PRICE_SOURCE) {
    return exact
  }
  if (normalizedProvider !== CUSTOM_PROVIDER) {
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
