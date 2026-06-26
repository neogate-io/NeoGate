export type ChannelProvider = string
export type EndpointProtocol = 'openai' | 'openai_oauth' | 'anthropic'
export type UserStatus = 'enabled' | 'disabled' | 'pending'
export type UserKeyStatus = 'enabled' | 'disabled'
export type ProjectStatus = 'enabled' | 'disabled'
export type BillingMeter = 'token' | 'image'

export type VersionCheckResult = {
  current_version: string
  latest_version: string
  latest_tag: string
  update_available: boolean
  release_url: string
  published_at?: string | null
}

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
  project_id: number
  project_name: string
  owner_user_id?: number | null
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
  username?: string | null
  status: UserStatus
  default_project_id?: number | null
  default_project_name?: string | null
  user_group_id: number
  user_group_code: string
  user_group_name: string
  user_key_count: number
  last_active_at?: string | null
  created_at: string
  updated_at: string
}

export type Project = CreditBalance & {
  id: number
  name: string
  owner_user_id: number
  owner_email: string
  owner_username?: string | null
  admin_display_names: string[]
  status: ProjectStatus
  is_default: boolean
  member_count: number
  user_key_count: number
  created_at: string
  updated_at: string
}

export type ProjectMember = {
  id: number
  project_id: number
  user_id: number
  user_email: string
  user_username?: string | null
  role: 'owner' | 'admin' | 'member' | 'viewer'
  user_status: UserStatus
  api_key?: string | null
  api_key_prefix?: string | null
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
  input_price_usd_micros?: number | null
  output_price_usd_micros?: number | null
  cache_read_price_usd_micros?: number | null
  cache_write_price_usd_micros?: number | null
  billing_meter?: BillingMeter | null
  unit_price_usd_micros?: number | null
  created_at: string
  updated_at: string
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

export type UsageRecord = {
  id: number
  user_id?: number | null
  user_email?: string | null
  user_username?: string | null
  user_key_id?: number | null
  channel_id?: number | null
  channel_key_id?: number | null
  credential_id?: number | null
  relay_trace_id?: string | null
  relay_attempt: number
  relay_final: boolean
  relay_path?: string | null
  relay_path_index?: number | null
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
  billing_meter: BillingMeter
  billable_units: number
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
  billing_meter: BillingMeter
  unit_price_usd_micros?: number | null
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
  input_price_usd_micros: number
  output_price_usd_micros: number
  cache_read_price_usd_micros?: number | null
  cache_write_price_usd_micros?: number | null
  billing_meter: BillingMeter
  unit_price_usd_micros?: number | null
  pricing_basis: BillingMeter
  source: string
  enabled: boolean
  created_at: string
  updated_at: string
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
  zpay_api_url: string
  zpay_merchant_id?: string | null
  zpay_secret_key_set: boolean
  zpay_default_pay_type: string
  zpay_site_name: string
  updated_at?: string | null
}

export type AppType = 'wecom' | 'webhook' | 'widget' | 'feishu' | 'dingtalk'
export type AppStatus = 'enabled' | 'disabled'
export type AppEndpointType = 'wecom' | 'webhook' | 'widget' | 'feishu' | 'dingtalk'

export type AppEndpoint = {
  id: number
  app_id: number
  endpoint_type: AppEndpointType
  name: string
  enabled: boolean
  config: Record<string, unknown>
  secrets_set: string[]
  callback_url?: string | null
  invoke_url?: string | null
  widget_script_url?: string | null
  last_active_at?: string | null
  created_at: string
  updated_at: string
}

export type AppRecord = {
  id: number
  name: string
  description: string
  app_type: AppType
  status: AppStatus
  model: string
  system_prompt: string
  context_turns: number
  max_output_tokens: number
  user_key_id: number
  user_key_name: string
  project_id: number
  project_name: string
  endpoint?: AppEndpoint | null
  today_message_count: number
  today_cost_micro_usd: number
  last_active_at?: string | null
  created_at: string
  updated_at: string
}

export type AppRunLog = {
  id: number
  app_id?: number | null
  endpoint_id?: number | null
  conversation_id?: number | null
  external_user_id?: string | null
  external_conversation_id?: string | null
  external_message_id?: string | null
  trace_id?: string | null
  app_type: AppType | string
  model?: string | null
  status: 'success' | 'failed' | 'duplicate' | 'ignored'
  status_code?: number | null
  latency_ms: number
  input_tokens?: number | null
  output_tokens?: number | null
  total_tokens?: number | null
  cost_micro_usd?: number | null
  error_summary?: string | null
  created_at: string
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
