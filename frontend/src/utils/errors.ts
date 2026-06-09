import type { MessageKey } from '../i18n'

export function readError(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}

type Translate = (key: MessageKey) => string

const smtpTestErrorMessageKeys = {
  smtp_authentication_failed: 'smtpAuthenticationFailed',
  smtp_connection_timed_out: 'smtpConnectionTimedOut',
  smtp_test_email_failed: 'smtpTestEmailFailed'
} as Record<string, MessageKey>

export function readSmtpTestError(err: unknown, t: Translate) {
  if (err instanceof ApiError && err.code) {
    const key = smtpTestErrorMessageKeys[err.code]
    if (key) return t(key)
  }

  return readError(err)
}

export function isSmtpConfigError(err: unknown) {
  if (!(err instanceof ApiError)) return false
  return Boolean(err.code && smtpConfigErrorCodes.has(err.code))
}

const smtpConfigErrorCodes = new Set([
  'smtp_settings_not_configured',
  'smtp_settings_invalid',
  'smtp_host_not_configured',
  'smtp_port_invalid',
  'smtp_sender_email_not_configured',
  'smtp_sender_email_invalid'
])

export function readApiErrorPayload(data: unknown) {
  if (typeof data !== 'object' || !data || !('error' in data)) {
    return
  }

  const error = (data as { error?: unknown }).error
  if (typeof error !== 'object' || !error) {
    return
  }

  const payload = error as { code?: unknown; message?: unknown }
  const code = typeof payload.code === 'string' ? payload.code : undefined
  const message = typeof payload.message === 'string' ? payload.message : undefined

  if (!code || !message) return
  return { message, code }
}

export class ApiError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly code?: string
  ) {
    super(message)
    this.name = 'ApiError'
  }
}
