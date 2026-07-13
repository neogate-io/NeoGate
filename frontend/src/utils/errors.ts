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
  invalid_email: 'invalidEmail',
  user_email_exists: 'userEmailAlreadyExists',
  upstream_timeout: 'genericUpstreamError',
  upstream_tls_error: 'genericUpstreamError',
  upstream_dns_error: 'genericUpstreamError',
  upstream_connect_error: 'genericUpstreamError',
  upstream_request_error: 'genericUpstreamError',
  upstream_unavailable: 'genericUpstreamError',
  pricing_reference_source_unavailable: 'referencePricesSourceUnavailable',
  database_password_invalid: 'databasePasswordInvalid',
  database_not_found: 'databaseNotFound',
  database_permission_denied: 'databasePermissionDenied',
  database_unavailable: 'databaseUnavailable',
  database_user_not_found: 'databaseUserNotFound',
  database_authentication_failed: 'databaseAuthenticationFailed',
  database_connection_failed: 'databaseConnectionFailed',
  database_connection_timeout: 'databaseConnectionTimeout',
  database_url_invalid: 'databaseUrlFormatInvalid',
  database_network_error: 'databaseNetworkError',
  database_tls_error: 'databaseTlsError',
  password_required: 'passwordRequired',
  current_password_required: 'passwordRequired',
  password_min_length: 'passwordMinLength',
  current_password_incorrect: 'currentPasswordIncorrect',
  password_same_as_current: 'passwordSameAsCurrent',
  registration_closed: 'registrationClosed',
  account_pending_approval: 'accountPendingApproval',
  verification_code_required: 'loginVerificationRequired',
  invalid_verification_code: 'loginVerificationInvalid',
  login_verification_rate_limited: 'loginVerificationRateLimited',
  password_reset_rate_limited: 'passwordResetRateLimited',
  price_model_required: 'priceModelRequired',
  price_must_be_non_negative: 'priceMustBeNonNegative',
  image_unit_price_required: 'imageUnitPriceRequired',
  video_billing_mode_required: 'videoBillingModeRequired',
  video_billing_meter_required: 'videoBillingMeterRequired',
  video_price_tiers_required: 'videoPriceTiersRequired',
  video_price_tier_resolution_required: 'videoPriceTierResolutionRequired',
  video_price_tier_price_required: 'videoPriceTierPriceRequired'
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

export function isNoModelsReturnedError(err: unknown) {
  return err instanceof ApiError && err.code === 'no_models_returned'
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
