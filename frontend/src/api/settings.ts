import type { SmtpSetting } from '../types/admin'
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
