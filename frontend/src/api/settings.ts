import type { PaymentSetting, SmtpSetting, VersionCheckResult } from '../types/admin'
import { adminRequest, publicRequest } from './request'

export type SiteSetting = {
  site_name: string
  public_base_url?: string | null
  logo_url?: string | null
  billing_currency?: 'USD' | 'CNY' | null
  env_write_supported: boolean
}

export type AdminModelSetting = {
  default_text_model?: string | null
  default_text_channel_id?: number | null
  default_text_channel_name?: string | null
  updated_at?: string | null
}

export function getPublicSiteSetting() {
  return publicRequest<SiteSetting>('/api/public/site')
}

export function getSiteSetting() {
  return adminRequest<SiteSetting>('/api/admin/settings/site')
}

export function saveSiteSetting(input: {
  site_name: string
  public_base_url: string
  logo_url?: string | null
}) {
  return adminRequest<{
    ok: boolean
    restart_required: boolean
    setting: SiteSetting
  }>('/api/admin/settings/site', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function getAdminModelSetting() {
  return adminRequest<AdminModelSetting>('/api/admin/settings/admin-model')
}

export function saveAdminModelSetting(input: {
  default_text_model?: string | null
  default_text_channel_id?: number | null
}) {
  return adminRequest<AdminModelSetting>('/api/admin/settings/admin-model', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function getSmtpSetting() {
  return adminRequest<SmtpSetting>('/api/admin/settings/smtp')
}

export function saveSmtpSetting(input: {
  smtp_host: string
  smtp_port: number
  smtp_username?: string | null
  smtp_password?: string | null
  clear_smtp_password: boolean
  smtp_tls: boolean
  from_email: string
  from_name?: string | null
  subject_prefix?: string | null
}) {
  return adminRequest<SmtpSetting>('/api/admin/settings/smtp', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function testSmtpSetting(input: {
  smtp_host: string
  smtp_port: number
  smtp_username?: string | null
  smtp_password?: string | null
  clear_smtp_password: boolean
  smtp_tls: boolean
  from_email: string
  from_name?: string | null
  subject_prefix?: string | null
}) {
  return adminRequest<{ ok: boolean }>('/api/admin/settings/smtp/test', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function getPaymentSetting() {
  return adminRequest<PaymentSetting>('/api/admin/settings/payment')
}

export function savePaymentSetting(input: {
  payment_enabled: boolean
  zpay_api_url: string
  zpay_merchant_id?: string | null
  zpay_secret_key?: string | null
  clear_zpay_secret_key: boolean
  zpay_default_pay_type: string
  zpay_site_name: string
}) {
  return adminRequest<PaymentSetting>('/api/admin/settings/payment', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function updateAdminPassword(input: {
  current_password: string
  new_password: string
}) {
  return adminRequest<{ ok: boolean }>('/api/admin/settings/admin-password', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export function checkLatestVersion() {
  return adminRequest<VersionCheckResult>('/api/admin/settings/version')
}
