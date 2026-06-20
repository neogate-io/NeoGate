import type { PaymentSetting, SmtpSetting, VersionCheckResult } from '../types/admin'
import { adminRequest } from './request'

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
  return_base_url?: string | null
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
