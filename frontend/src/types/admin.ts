export type ChannelProvider = string
export type EndpointProtocol = 'openai' | 'openai_oauth' | 'anthropic'
export type UserStatus = 'enabled' | 'disabled' | 'pending'
export type UserKeyStatus = 'enabled' | 'disabled'

export type CursorPage<T> = {
  items: T[]
  limit: number
  next_cursor?: string | null
  has_more?: boolean
}

export type CreditBalance = {
  balance_micro_usd: number
  reserved_micro_usd: number
  available_micro_usd: number
}

export type UserKey = CreditBalance & {
  id: number
  user_id: number
  name: string
  key: string
  key_prefix: string
  status: UserKeyStatus
  last_active_at?: string | null
  expires_at?: string | null
  model_limits?: string[] | null
  month_cost_micro_usd: number
  created_at: string
  updated_at: string
}

export type UserGroup = {
  id: number
  code: string
  name: string
  is_default: boolean
  enabled: boolean
  user_count: number
  created_at: string
  updated_at: string
}

export type User = CreditBalance & {
  id: number
  email: string
  status: UserStatus
  user_group_id: number
  user_group_code: string
  user_group_name: string
  user_key_count: number
  last_active_at?: string | null
  created_at: string
  updated_at: string
}

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

export type ProviderRecord = {
  id: number
  code: ChannelProvider
  display_name: string
  name: string
  default_models: string[]
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
  key_prefix: string
  enabled: boolean
  healthy: boolean
  cooldown_until?: string | null
  last_error?: string | null
  last_used_at?: string | null
  created_at: string
  updated_at: string
}

export type UsageRecord = {
  id: number
  user_id?: number | null
  user_key_id?: number | null
  channel_id?: number | null
  channel_key_id?: number | null
  provider: string
  model?: string | null
  status_code?: number | null
  streamed: boolean
  error_summary?: string | null
  latency_ms: number
  first_response_ms?: number | null
  output_tokens_per_second?: number | null
  input_tokens?: number | null
  output_tokens?: number | null
  total_tokens?: number | null
  cache_in_tokens?: number | null
  cache_create_in_tokens?: number | null
  cache_create_5m_in_tokens?: number | null
  cache_create_1h_in_tokens?: number | null
  reason_out_tokens?: number | null
  audio_in_tokens?: number | null
  audio_out_tokens?: number | null
  cost_micro_usd?: number | null
  billing_status: string
  created_at: string
}

export type ProviderPrice = {
  id: number
  provider: string
  model: string
  input_price_usd_micros: number
  output_price_usd_micros: number
  cache_read_price_usd_micros?: number | null
  cache_write_price_usd_micros?: number | null
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
  enabled: boolean
  discovered_at: string
  created_at: string
  updated_at: string
}

export type PricingTemplate = {
  id: number
  provider: string
  model: string
  input_price_usd_micros: number
  output_price_usd_micros: number
  cache_read_price_usd_micros?: number | null
  cache_write_price_usd_micros?: number | null
  source: string
  enabled: boolean
  created_at: string
  updated_at: string
}

export type PricingTemplateSyncResult = {
  source: string
  fetched: number
  saved: number
  skipped: number
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

export type SmtpSetting = {
  configured: boolean
  smtp_host: string
  smtp_port: number
  smtp_username?: string | null
  smtp_password_set: boolean
  smtp_tls: boolean
  from_email: string
  from_name?: string | null
  subject_prefix?: string | null
  updated_at?: string | null
}

export type PaymentSetting = {
  configured: boolean
  payment_enabled: boolean
  return_base_url?: string | null
  zpay_api_url: string
  zpay_merchant_id?: string | null
  zpay_secret_key_set: boolean
  zpay_default_pay_type: string
  zpay_site_name: string
  updated_at?: string | null
}

export type CredentialQuotaWindow = {
  percent?: number | null
  used?: number | null
  limit?: number | null
  reset_at?: string | null
}

export type CredentialQuota = {
  status: 'ok' | 'failed' | 'unavailable' | string
  message?: string | null
  plan?: string | null
  five_hour?: CredentialQuotaWindow | null
  weekly?: CredentialQuotaWindow | null
  updated_at: string
}

export type Credential = {
  provider: 'openai' | string
  id: number
  identity_label?: string | null
  filename: string
  enabled: boolean
  auth_mode?: string | null
  api_key_preview?: string | null
  has_oauth_tokens: boolean
  has_refresh_token: boolean
  has_id_token: boolean
  email?: string | null
  account_id?: string | null
  last_refresh?: string | null
  updated_at?: string | null
  quota?: CredentialQuota | null
}

export type CredentialUploadFailure = {
  filename: string
  error: string
}

export type CredentialUploadResult = {
  imported: Credential[]
  failed: CredentialUploadFailure[]
}

export type OpenAICredential = Credential
