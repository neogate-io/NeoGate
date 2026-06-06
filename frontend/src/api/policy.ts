import { adminRequest, publicRequest, userRequest } from './request'
import type { PaymentSetting, ProviderRecord, SmtpSetting } from '../types/admin'

export type ServiceMode = 'internal' | 'paid'

export type ServicePolicy = {
  runtime_mode: 'standalone' | 'distributed'
  env_write_supported: boolean
  database_configured: boolean
  database_connected: boolean
  redis_configured: boolean
  redis_connected?: boolean | null
  secrets_configured: boolean
  site_configured: boolean
  setup_completed: boolean
  bootstrap_required: boolean
  bootstrap_blocked_reason?: 'cluster_requires_external_config' | 'missing_database' | 'missing_redis' | null
  restart_required: boolean
  site_name?: string | null
  public_base_url?: string | null
  service_mode: ServiceMode
  credit_required: boolean
  recharge_enabled: boolean
  updated_at?: string | null
}

export function getSetupStatus() {
  return publicRequest<ServicePolicy>('/api/setup/status')
}

export function completeSetup(serviceMode: ServiceMode) {
  return publicRequest<ServicePolicy>('/api/setup', {
    method: 'POST',
    body: JSON.stringify({ service_mode: serviceMode })
  })
}

export function bootstrapSetup(input: {
  setup_token: string
  database_url?: string | null
  site_name?: string | null
  public_base_url?: string | null
}) {
  return publicRequest<{ ok: boolean; env_file: string; restart_required: boolean }>(
    '/api/setup/bootstrap',
    {
      method: 'POST',
      body: JSON.stringify(input)
    }
  )
}

export function getClusterEnvTemplate() {
  return publicRequest<{
    env_text: string
    generated_admin_token_secret?: string | null
    generated_upstream_secret_key?: string | null
    required_restart: boolean
  }>('/api/setup/cluster-env-template', {
    method: 'POST'
  })
}

export function getSetupProviders() {
  return publicRequest<ProviderRecord[]>('/api/setup/providers')
}

export function fetchSetupUpstreamModels(input: {
  provider: string
  protocol: 'openai' | 'anthropic'
  base_url: string
  secret: string
}) {
  return publicRequest<{ models: string[] }>('/api/setup/upstream-models', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function completeSetupWizard(input: {
  admin_password: string
  service_mode: ServiceMode
  credit_required?: boolean
  channel: {
    provider: string
    name: string
    protocol: 'openai' | 'anthropic'
    base_url: string
    models: string[]
    secret: string
  }
  prices: Array<{
    provider: string
    model: string
    input_price_usd_micros: number
    output_price_usd_micros: number
    enabled: boolean
  }>
  smtp?: Partial<SmtpSetting> & {
    smtp_password?: string | null
    clear_smtp_password?: boolean
  } | null
  payment?: Partial<PaymentSetting> & {
    zpay_secret_key?: string | null
    clear_zpay_secret_key?: boolean
  } | null
}) {
  return publicRequest<ServicePolicy>('/api/setup/complete', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function getUserServicePolicy() {
  return userRequest<ServicePolicy>('/api/user/service-policy')
}

export function getAdminServicePolicy() {
  return adminRequest<ServicePolicy>('/api/admin/settings/service-policy')
}

export function saveAdminServicePolicy(input: { credit_required: boolean }) {
  return adminRequest<ServicePolicy>('/api/admin/settings/service-policy', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}
