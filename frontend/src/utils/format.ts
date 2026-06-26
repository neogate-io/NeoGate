export const MICRO_USD_PER_USD = 1_000_000

export type CacheWriteTokenSource = {
  cache_create_in_tokens?: number | null
  cache_create_5m_in_tokens?: number | null
  cache_create_1h_in_tokens?: number | null
}

export function microUsdToUsd(value: number) {
  return value / MICRO_USD_PER_USD
}

export function usdToMicroUsd(value: number) {
  return Math.round(value * MICRO_USD_PER_USD)
}

export function formatMicroUsd(value?: number | null, digits = 2) {
  if (value == null) return '-'
  return `$${microUsdToUsd(value).toFixed(digits)}`
}

export function formatUsdPerMillion(value: number) {
  return `$${value.toLocaleString('en-US', {
    maximumFractionDigits: 6
  })}`
}

export function formatMicrosPerMillion(value?: number | null) {
  if (value == null) return '-'
  return formatUsdPerMillion(microUsdToUsd(value))
}

export function formatNumber(value: number | null | undefined, locale: string) {
  return value == null ? '-' : value.toLocaleString(locale)
}

export function formatDateTime(
  value: string | null | undefined,
  locale: string,
  options?: Intl.DateTimeFormatOptions
) {
  return value ? new Date(value).toLocaleString(locale, options) : '-'
}

export function formatCompactDateTime(value?: string | null) {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '-'
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  const hour = String(date.getHours()).padStart(2, '0')
  const minute = String(date.getMinutes()).padStart(2, '0')
  return `${year}/${month}/${day} ${hour}:${minute}`
}

export function maskApiKey(value: string) {
  if (!value || value.includes('*')) return value
  if (value.length <= 18) return value
  return `${value.slice(0, 8)}********${value.slice(-6)}`
}

export function formatDurationMs(ms?: number | null) {
  if (ms == null) return '-'
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`
}

export function formatTokenRate(
  value: number | null | undefined,
  locale: string,
  emptyValue = '-'
) {
  if (value == null || value <= 0) return emptyValue
  return `${Math.round(value).toLocaleString(locale)} t/s`
}

export function cacheWriteTokens(row: CacheWriteTokenSource) {
  const split = (row.cache_create_5m_in_tokens || 0) + (row.cache_create_1h_in_tokens || 0)
  return split > 0 ? split : row.cache_create_in_tokens || 0
}

export function downloadCsv(filename: string, rows: Array<Array<string | number>>) {
  const csv = rows.map((row) => row.map(escapeCsvValue).join(',')).join('\n')
  const blob = new Blob([`\uFEFF${csv}`], { type: 'text/csv;charset=utf-8' })
  downloadBlob(filename, blob)
}

export function downloadBlob(filename: string, blob: Blob) {
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.click()
  URL.revokeObjectURL(url)
}

export function escapeCsvValue(value: string | number) {
  const text = String(value)
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
}

export function toDateKey(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}
