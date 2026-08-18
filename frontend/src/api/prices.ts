import type {
  BillingMeter,
  ModelReferenceCatalogRecord,
  PricingPolicy,
  PricingTemplate,
  PricingTemplateSyncResult,
  ProviderModel,
  ChannelPrice,
  VideoBillingMode,
  VideoPriceTier
} from '../types/admin'
import { adminRequest } from './request'

export function getProviderModels() {
  return adminRequest<ProviderModel[]>('/api/admin/provider-models')
}

export function getChannelPrices() {
  return adminRequest<ChannelPrice[]>('/api/admin/channel-prices')
}

export function getPricingTemplates() {
  return adminRequest<PricingTemplate[]>('/api/admin/pricing-templates')
}

export function getModelReferenceCatalog() {
  return adminRequest<ModelReferenceCatalogRecord[]>('/api/admin/model-reference-catalog')
}

export function getLiveModelReferenceCatalog() {
  return adminRequest<ModelReferenceCatalogRecord[]>('/api/admin/model-reference-catalog/live')
}

export function syncPricingTemplates(source = '') {
  return adminRequest<PricingTemplateSyncResult>('/api/admin/pricing-templates/sync', {
    method: 'POST',
    body: JSON.stringify({ source })
  })
}

export function getPricingPolicies() {
  return adminRequest<PricingPolicy[]>('/api/admin/pricing-policies')
}

export function upsertChannelPrice(input: {
  channel_id: number
  model: string
  input_price_micros: number
  output_price_micros: number
  cache_read_price_micros?: number | null
  cache_write_price_micros?: number | null
  billing_meter: BillingMeter
  unit_price_micros?: number | null
  video_billing_mode?: VideoBillingMode | null
  video_price_tiers?: VideoPriceTier[]
  enabled: boolean
}) {
  return adminRequest<ChannelPrice>('/api/admin/channel-prices', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function upsertPricingPolicy(input: {
  id?: number
  name: string
  user_group?: string | null
  multiplier_micros: number
  enabled: boolean
  priority: number
}) {
  return adminRequest<PricingPolicy>('/api/admin/pricing-policies', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}
