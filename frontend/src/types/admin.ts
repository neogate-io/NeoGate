import type { BillingMeter, VideoBillingMode } from './channel'

export type UserStatus = 'enabled' | 'disabled' | 'pending'
export type UserKeyStatus = 'enabled' | 'disabled'
export type ProjectStatus = 'enabled' | 'disabled'
export type {
  BillingMeter,
  Channel,
  ChannelEndpoint,
  ChannelKey,
  ChannelModel,
  ChannelPrice,
  ChannelProbeSample,
  ChannelProvider,
  ChannelDiagnosticReport,
  DiagnosticStatus,
  DiagnosticStep,
  EndpointDiagnosticReport,
  EndpointProtocol,
  KeyDiagnosticReport,
  ModelReferenceCatalogRecord,
  PricingBasis,
  PricingPolicy,
  PricingTemplate,
  PricingTemplateSyncResult,
  ProviderDefaultEndpoint,
  ProviderModel,
  ProviderRecord,
  VideoBillingMode,
  VideoPriceTier,
  VideoTier,
  VideoTierDimension
} from './channel'

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
  balance_micros: number
  reserved_micros: number
  available_micros: number
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
  month_cost_micros: number
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
  project_model_count: number
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

export type ProjectModelRoutingTaskType =
  | 'chat'
  | 'code'
  | 'reasoning'
  | 'translation'
  | 'summarization'
  | 'extraction'
  | 'structured_output'
  | 'tool_use'
  | 'vision'
  | 'long_context'
  | 'unknown'

export type RoutingMatchedRule = {
  id: string
  category: string
  weight: number
  reason: string
}

export type RoutingCandidateScore = {
  candidate_id: number
  target_model: string
  tier: ProjectModelCandidateTier
  priority: number
  weight: number
  score: number
  reason: string
}

export type RoutingDecision = {
  id: number
  project_id: number
  project_model_id?: number | null
  requested_model: string
  selected_model: string
  selected_channel_id?: number | null
  decision_source: 'rules' | 'classifier' | 'fallback' | string
  tier: ProjectModelCandidateTier
  task_type: ProjectModelRoutingTaskType | string
  confidence: number
  reason: string
  matched_rules: RoutingMatchedRule[]
  candidate_scores: RoutingCandidateScore[]
  fallback_reason?: string | null
  classifier_model?: string | null
  latency_ms: number
  created_at: string
}

export type UsageRouting = {
  id: number
  project_id: number
  project_model_id?: number | null
  requested_model: string
  selected_model: string
  selected_channel_id?: number | null
  decision_source: 'rules' | 'classifier' | 'fallback' | string
  tier: ProjectModelCandidateTier
  task_type: ProjectModelRoutingTaskType | string
  confidence: number
  reason_code: string
  matched_rule_ids: string[]
  candidate_summary: Array<{
    target_model: string
    tier: ProjectModelCandidateTier
    priority: number
    weight: number
  }>
  fallback_reason?: string | null
  classifier_model?: string | null
  latency_ms: number
  created_at: string
}

export type ProjectModel = {
  id: number
  project_id: number
  model: string
  target_model: string
  target_channel_id?: number | null
  target_channel_name?: string | null
  route_mode: 'direct' | 'smart'
  routing_config: ProjectModelRoutingConfig
  candidates: ProjectModelCandidate[]
  enabled: boolean
  description: string
  created_at: string
  updated_at: string
}

export type ProjectModelRoutingConfig = {
  smart_model_name: string
  default_tier: ProjectModelCandidateTier
  low_confidence_threshold: number
  classifier_enabled: boolean
  classifier_model?: string | null
}

export type ProjectModelCandidateTier = 'simple' | 'standard' | 'advanced'

export type ProjectModelCandidate = {
  id: number
  project_model_id: number
  target_model: string
  target_channel_id?: number | null
  target_channel_name?: string | null
  tier: ProjectModelCandidateTier
  priority: number
  weight: number
  enabled: boolean
  created_at: string
  updated_at: string
}

export type AutoSuggestion = {
  tier: ProjectModelCandidateTier
  target_model: string
  target_channel_id?: number | null
  target_channel_name?: string | null
  reason: string
}

export type AutoConfigResponse = {
  suggestions: AutoSuggestion[]
  warnings: string[]
  source: 'llm' | 'rules' | string
}

export type UsageRecord = {
  id: number
  user_id?: number | null
  user_email?: string | null
  user_username?: string | null
  user_key_id?: number | null
  channel_id?: number | null
  channel_name?: string | null
  channel_key_id?: number | null
  credential_id?: number | null
  relay_trace_id?: string | null
  relay_attempt: number
  relay_final: boolean
  relay_path?: string | null
  relay_path_index?: number | null
  model?: string | null
  upstream_model?: string | null
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
  cost_micros?: number | null
  billing_status: string
  video_billing?: {
    mode: VideoBillingMode
    resolution: string
    duration_seconds: number
    has_video_input: boolean
    price_micros: number
  } | null
  routing?: UsageRouting | null
  created_at: string
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
  today_cost_micros: number
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
  cost_micros?: number | null
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
