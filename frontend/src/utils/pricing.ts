import type { PricingTemplate, ChannelPrice } from '../types/admin'

const CONFIRMED_PRICE_SOURCE = 'confirmed_price'

export function priceKey(provider: string, model: string) {
  return `${provider.trim()}\u0000${model.trim().toLowerCase()}`
}

export function channelPriceKey(channelId: number, model: string) {
  return `${channelId}\u0000${model.trim().toLowerCase()}`
}

export function derivedCacheReadPrice(inputPrice: number) {
  return Math.round(inputPrice / 10)
}

export function estimatedVideoTokensPerSecond(resolutionsText?: string | null) {
  const normalized = (resolutionsText ?? '').trim().toLowerCase()
  return /(^|[^a-z0-9])(?:1080p|2160p|4k)(?=$|[^a-z0-9])/.test(normalized) ? 200_000 : 100_000
}

export function resolvedVideoTokensPerSecondEstimate(
  estimatedTokensPerSecond: number | null | undefined,
  resolutionsText?: string | null
) {
  if (
    estimatedTokensPerSecond &&
    Number.isFinite(estimatedTokensPerSecond) &&
    estimatedTokensPerSecond > 0 &&
    estimatedTokensPerSecond !== 1_000_000
  ) {
    return estimatedTokensPerSecond
  }
  return estimatedVideoTokensPerSecond(resolutionsText)
}

export function isChannelPriceConfigured(price?: ChannelPrice) {
  if (!price) return false
  if (price.billing_meter === 'image') {
    return price.unit_price_micros !== undefined && price.unit_price_micros !== null
  }
  if (price.billing_meter === 'video') {
    return Boolean(price.video_billing_mode && price.video_price_tiers.length > 0)
  }
  return price.input_price_micros >= 0 && price.output_price_micros >= 0
}

export function isChannelPriceReady(price?: ChannelPrice) {
  return Boolean(price?.enabled && isChannelPriceConfigured(price))
}

export function pricingReferenceModelAliases(model: string) {
  const aliases = new Set<string>()
  const queue = [model.trim().toLowerCase()]

  for (const alias of queue) {
    if (!alias || aliases.has(alias)) continue
    aliases.add(alias)

    const dotVersionAlias = alias.replace(/-(\d+)-(\d+)(?=-|$)/g, '-$1.$2')
    if (dotVersionAlias !== alias) queue.push(dotVersionAlias)

    const withoutDateSuffix = alias.replace(/-\d{6}$/, '')
    if (withoutDateSuffix !== alias) queue.push(withoutDateSuffix)

    const withoutResolutionSuffix = alias.replace(/-(?:480p|720p|1080p|4k)$/, '')
    if (withoutResolutionSuffix !== alias) queue.push(withoutResolutionSuffix)
  }

  return aliases
}

function hasReferenceModelAlias(model: string, templateModel: string) {
  const aliases = pricingReferenceModelAliases(model)
  return [...pricingReferenceModelAliases(templateModel)].some((alias) => aliases.has(alias))
}

export function findPricingTemplate(templates: PricingTemplate[], provider: string, model: string) {
  const normalizedProvider = provider.trim()
  const normalizedModel = model.trim().toLowerCase()
  const enabledTemplates = templates.filter(
    (template) =>
      template.enabled &&
      template.source !== CONFIRMED_PRICE_SOURCE &&
      hasReferenceModelAlias(normalizedModel, template.model)
  )
  const sameModelTemplates = enabledTemplates.filter(
    (template) => template.model.trim().toLowerCase() === normalizedModel
  )
  const exact = sameModelTemplates.find(
    (template) => template.provider.trim() === normalizedProvider
  )

  if (exact) {
    return exact
  }
  return (
    enabledTemplates.find((template) => template.provider.trim() === normalizedProvider) ??
    sameModelTemplates.find((template) => template.provider.trim() !== normalizedProvider) ??
    enabledTemplates.find((template) => template.provider.trim() !== normalizedProvider)
  )
}
