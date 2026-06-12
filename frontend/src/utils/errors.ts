import { defaultLocale, isLocale, translate, type MessageKey } from '../i18n'

export function readError(err: unknown) {
  if (err instanceof ApiError) {
    const mapped = readMappedApiError(err, generalApiErrorMessageKeys)
    if (mapped) return mapped
    if (err.status >= 500 || isGenericServerMessage(err.message)) {
      return tError('genericServerError')
    }

    return err.message || tError('genericRequestError')
  }

  if (err instanceof TypeError && isNetworkErrorMessage(err.message)) {
    return tError('genericNetworkError')
  }

  if (err instanceof Error) {
    return isNetworkErrorMessage(err.message) ? tError('genericNetworkError') : err.message
  }

  return String(err || tError('genericRequestError'))
}

type Translate = (key: MessageKey) => string

const localeStorageKey = 'neogate_locale'

const generalApiErrorMessageKeys = {
  internal_server_error: 'genericServerError',
  upstream_timeout: 'genericUpstreamError',
  upstream_tls_error: 'genericUpstreamError',
  upstream_dns_error: 'genericUpstreamError',
  upstream_connect_error: 'genericUpstreamError',
  upstream_request_error: 'genericUpstreamError',
  upstream_unavailable: 'genericUpstreamError'
} as Record<string, MessageKey>

function tError(key: MessageKey) {
  const locale =
    typeof localStorage === 'undefined' ? defaultLocale : localStorage.getItem(localeStorageKey)
  return translate(isLocale(locale) ? locale : defaultLocale, key)
}

function isGenericServerMessage(message: string) {
  const normalized = message.trim().toLowerCase().replace(/[_-]+/g, ' ')
  return normalized === 'internal server error' || normalized === 'server error'
}

function isNetworkErrorMessage(message: string) {
  const normalized = message.trim().toLowerCase()
  return (
    normalized === 'failed to fetch' ||
    normalized === 'load failed' ||
    normalized.includes('networkerror') ||
    normalized.includes('network request failed')
  )
}

const smtpTestErrorMessageKeys = {
  smtp_authentication_failed: 'smtpAuthenticationFailed',
  smtp_connection_timed_out: 'smtpConnectionTimedOut',
  smtp_test_email_failed: 'smtpTestEmailFailed'
} as Record<string, MessageKey>

export function readSmtpTestError(err: unknown, t: Translate) {
  const mapped = readMappedApiError(err, smtpTestErrorMessageKeys, t)
  if (mapped) return mapped

  return readError(err)
}

const modelFetchErrorMessageKeys = {
  upstream_timeout: 'modelsFetchUpstreamTimeout',
  upstream_tls_error: 'modelsFetchUpstreamTlsError',
  upstream_dns_error: 'modelsFetchUpstreamDnsError',
  upstream_connect_error: 'modelsFetchUpstreamUnavailable',
  upstream_request_error: 'modelsFetchUpstreamUnavailable',
  upstream_unavailable: 'modelsFetchUpstreamUnavailable',
  internal_server_error: 'modelsFetchUpstreamUnavailable'
} as Record<string, MessageKey>

export function readModelFetchError(err: unknown, t: Translate) {
  const mapped = readMappedApiError(err, modelFetchErrorMessageKeys, t)
  if (mapped) return mapped

  return readError(err)
}

function readMappedApiError(
  err: unknown,
  messages: Record<string, MessageKey>,
  t: Translate = tError
) {
  if (!(err instanceof ApiError) || !err.code) return ''
  const key = messages[err.code]
  return key ? t(key) : ''
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
