export type ChannelProvider = string
export type EndpointProtocol = 'openai' | 'openai_oauth' | 'anthropic'
export type BillingMeter = 'token' | 'image' | 'video' | 'audio'
export type VideoBillingMode = 'official_token' | 'per_second'
export type VideoPriceTier = {
  resolutions: string[]
  input_with_video_micros?: number | null
  input_without_video_micros?: number | null
  estimated_tokens_per_second?: number | null
  input_with_video_unit_micros?: number | null
  input_without_video_unit_micros?: number | null
}
export type PricingBasis =
  | 'token'
  | 'image'
  | 'call'
  | 'per_10k_token'
  | 'hour'
  | 'second'
  | 'multi_tier_video'

export type Channel = {
  id: number
  provider: ChannelProvider
  name: string
  enabled: boolean
  priority: number
  weight: number
  key_selection_mode: 'polling' | 'random'
  use_credentials: boolean
  endpoints: ChannelEndpoint[]
  models: ChannelModel[]
  probe_samples: ChannelProbeSample[]
  created_at?: string
  updated_at?: string
}

export type ChannelEndpoint = {
  id: number
  channel_id: number
  protocol: EndpointProtocol
  base_url: string
  models: string[]
  enabled: boolean
  healthy: boolean
  last_error?: string | null
  cooldown_until?: string | null
  created_at?: string
  updated_at?: string
}

export type ChannelModel = {
  id: number
  channel_id: number
  provider: string
  model: string
  base_model?: string | null
  enabled: boolean
  status: 'available' | 'missing' | 'disabled'
  runtime_status: 'normal' | 'cooldown' | 'failed'
  cooldown_until?: string | null
  last_seen_at?: string | null
  missing_since?: string | null
  last_probe_at?: string | null
  last_error?: string | null
  last_status_code?: number | null
  success_count: number
  failure_count: number
  billing_enabled: boolean
  price_configured: boolean
  input_price_micros?: number | null
  output_price_micros?: number | null
  cache_read_price_micros?: number | null
  cache_write_price_micros?: number | null
  billing_meter?: BillingMeter | null
  unit_price_micros?: number | null
  created_at: string
  updated_at: string
}

export type ProviderRecord = {
  id: number
  code: ChannelProvider
  display_name: string
  name: string
  default_endpoints: ProviderDefaultEndpoint[]
  enabled: boolean
  sort_order: number
  created_at?: string
  updated_at?: string
}

export type ProviderDefaultEndpoint = {
  protocol: EndpointProtocol
  base_url: string
}

export type ChannelKey = {
  id: number
  channel_id: number
  name: string
  masked_key: string
  enabled: boolean
  healthy: boolean
  cooldown_until?: string | null
  last_error?: string | null
  last_used_at?: string | null
  created_at: string
  updated_at: string
}

export type ChannelProbeSample = {
  status: 'ok' | 'failed' | 'skipped'
  latency_ms?: number | null
  status_code?: number | null
  model: string
  error_summary?: string | null
  created_at: string
}

export type DiagnosticStatus = 'ok' | 'warning' | 'failed' | 'skipped'

export type DiagnosticStep = {
  name: string
  status: DiagnosticStatus
  message: string
  duration_ms: number
  status_code?: number | null
}

export type KeyDiagnosticReport = {
  key_id?: number | null
  key_name: string
  masked_key?: string | null
  key_prefix?: string | null
  status: DiagnosticStatus
  summary: string
  discovered_models: string[]
  steps: DiagnosticStep[]
}

export type EndpointDiagnosticReport = {
  endpoint_id: number
  protocol: EndpointProtocol
  base_url: string
  status: DiagnosticStatus
  summary: string
  discovered_models: string[]
  configured_models: string[]
  missing_configured_models: string[]
  keys: KeyDiagnosticReport[]
}

export type ChannelDiagnosticReport = {
  channel_id: number
  channel_name: string
  provider: string
  status: DiagnosticStatus
  summary: string
  started_at: string
  finished_at: string
  duration_ms: number
  endpoints: EndpointDiagnosticReport[]
}

export type ChannelPrice = {
  id: number
  channel_id: number
  provider: string
  model: string
  input_price_micros: number
  output_price_micros: number
  cache_read_price_micros?: number | null
  cache_write_price_micros?: number | null
  billing_meter: BillingMeter
  unit_price_micros?: number | null
  video_billing_mode?: VideoBillingMode | null
  video_price_tiers: VideoPriceTier[]
  enabled: boolean
  created_at: string
  updated_at: string
}

export type ProviderModel = {
  id: number
  provider: string
  model: string
  display_name: string
  source: 'seed' | 'upstream' | 'channel'
  billing_meter: BillingMeter
  capabilities: Record<string, unknown>
  enabled: boolean
  discovered_at: string
  created_at: string
  updated_at: string
}

export type PricingTemplate = {
  id: number
  provider: string
  model: string
  input_price_micros: number
  output_price_micros: number
  cache_read_price_micros?: number | null
  cache_write_price_micros?: number | null
  billing_meter: BillingMeter
  unit_price_micros?: number | null
  pricing_basis: PricingBasis
  source: string
  enabled: boolean
  created_at: string
  updated_at: string
}

export type VideoTierDimension =
  | 'input_without_video'
  | 'input_with_video'
  | 'with_audio'
  | 'without_audio'
  | 'price'

export type VideoTier = {
  resolution: string
  label?: string
  unit?: string
  tiers: Partial<Record<VideoTierDimension, number>>
}

export type ModelReferenceCatalogRecord = PricingTemplate & {
  display_name: string
  capabilities: Record<string, unknown>
  model_source: string
  model_updated_at: string
}

export type PricingTemplateSyncResult = {
  source: string
  fetched: number
  saved: number
  skipped: number
  removed: number
}

export type PricingPolicy = {
  id: number
  name: string
  user_group?: string | null
  multiplier_micros: number
  enabled: boolean
  priority: number
  created_at: string
  updated_at: string
}
