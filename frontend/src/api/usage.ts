import type { CursorPage, UsageRecord } from '../types/admin'
import { adminFileRequest, adminRequest, userRequest } from './request'

export type UsagePage = CursorPage<UsageRecord> & {
  total: number
  page: number
}

export type AdminUsageStatus = 'all' | 'success' | 'failed'

export type AdminUsageQuery = {
  page?: number
  limit?: number
  start?: string
  end?: string
  query?: string
  model?: string
  status?: AdminUsageStatus
  cursor?: string
}

export type UsageStatisticsSort = 'cost_desc' | 'tokens_desc' | 'requests_desc'
export type UsageStatisticsExportScope = 'users' | 'user_models' | 'daily' | 'models'

export type UsageStatisticsQuery = {
  start?: string
  end?: string
  user_id?: number
  user_query?: string
  provider?: string
  model?: string
  billing_meter?: 'token' | 'image'
  page?: number
  limit?: number
  sort?: UsageStatisticsSort
}

export type UsageStatisticsAggregate = {
  request_count: number
  success_count: number
  error_count: number
  streamed_count: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  cache_in_tokens: number
  cache_write_tokens: number
  reason_out_tokens: number
  audio_in_tokens: number
  audio_out_tokens: number
  billable_units: number
  cost_micro_usd: number
  avg_latency_ms?: number | null
  avg_first_response_ms?: number | null
}

export type DailyUsageStatistics = {
  date: string
  request_count: number
  success_count: number
  error_count: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  billable_units: number
  cost_micro_usd: number
  avg_latency_ms?: number | null
}

export type UserUsageStatistics = {
  user_id?: number | null
  user_email?: string | null
  user_username?: string | null
  user_display_name: string
  request_count: number
  success_count: number
  error_count: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  billable_units: number
  cost_micro_usd: number
  avg_latency_ms?: number | null
  model_count: number
}

export type ModelUsageStatistics = {
  provider: string
  model: string
  billing_meter: 'token' | 'image'
  request_count: number
  success_count: number
  error_count: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  billable_units: number
  cost_micro_usd: number
  avg_latency_ms?: number | null
  user_count: number
}

export type UserModelUsageStatistics = {
  user_id?: number | null
  user_email?: string | null
  user_username?: string | null
  user_display_name: string
  provider: string
  model: string
  billing_meter: 'token' | 'image'
  request_count: number
  success_count: number
  error_count: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  billable_units: number
  cost_micro_usd: number
  avg_latency_ms?: number | null
}

export type ProviderUsageStatistics = {
  provider: string
  request_count: number
  total_tokens: number
  billable_units: number
  cost_micro_usd: number
}

export type UsageStatisticsSummary = {
  start: string
  end: string
  totals: UsageStatisticsAggregate
  daily: DailyUsageStatistics[]
  top_users: UserUsageStatistics[]
  top_models: ModelUsageStatistics[]
  providers: ProviderUsageStatistics[]
}

export type UsageStatisticsPage<T> = {
  items: T[]
  total: number
  page: number
  limit: number
}

export type UsageStatisticsOptions = {
  providers: string[]
  models: Array<{ provider: string; model: string }>
  users: Array<{
    user_id: number
    user_email?: string | null
    user_username?: string | null
    user_display_name: string
  }>
}

export function getAdminUsage(query: AdminUsageQuery = {}) {
  const params = new URLSearchParams({
    page: String(query.page ?? 1),
    limit: String(query.limit ?? 20)
  })
  if (query.start) params.set('start', query.start)
  if (query.end) params.set('end', query.end)
  if (query.query) params.set('query', query.query)
  if (query.model) params.set('model', query.model)
  if (query.status && query.status !== 'all') params.set('status', query.status)
  if (query.cursor) params.set('cursor', query.cursor)
  return adminRequest<UsagePage>(`/api/admin/usage?${params}`)
}

export function getAdminUsageStatisticsSummary(query: UsageStatisticsQuery = {}) {
  return adminRequest<UsageStatisticsSummary>(
    `/api/admin/usage/statistics/summary?${usageStatisticsParams(query)}`
  )
}

export function getAdminUsageStatisticsUsers(query: UsageStatisticsQuery = {}) {
  return adminRequest<UsageStatisticsPage<UserUsageStatistics>>(
    `/api/admin/usage/statistics/users?${usageStatisticsParams(query)}`
  )
}

export function getAdminUsageStatisticsUserModels(query: UsageStatisticsQuery = {}) {
  return adminRequest<UsageStatisticsPage<UserModelUsageStatistics>>(
    `/api/admin/usage/statistics/user-models?${usageStatisticsParams(query)}`
  )
}

export function getAdminUsageStatisticsModels(query: UsageStatisticsQuery = {}) {
  return adminRequest<UsageStatisticsPage<ModelUsageStatistics>>(
    `/api/admin/usage/statistics/models?${usageStatisticsParams(query)}`
  )
}

export function getAdminUsageStatisticsOptions(query: UsageStatisticsQuery = {}) {
  return adminRequest<UsageStatisticsOptions>(
    `/api/admin/usage/statistics/options?${usageStatisticsParams(query)}`
  )
}

export function downloadAdminUsageStatisticsCsv(
  scope: UsageStatisticsExportScope,
  query: UsageStatisticsQuery = {}
) {
  const params = usageStatisticsParams(query)
  params.set('scope', scope)
  return adminFileRequest(`/api/admin/usage/statistics/export.csv?${params}`)
}

export function downloadAdminUsageCsv(query: AdminUsageQuery = {}) {
  const params = adminUsageParams(query)
  return adminFileRequest(`/api/admin/usage/export.csv?${params}`)
}

export function getUserUsage(page = 1, limit = 20, start?: string, end?: string, cursor?: string) {
  const params = new URLSearchParams({
    page: String(page),
    limit: String(limit)
  })
  if (start) params.set('start', start)
  if (end) params.set('end', end)
  if (cursor) params.set('cursor', cursor)
  return userRequest<UsagePage>(`/api/user/usage?${params}`)
}

function adminUsageParams(query: AdminUsageQuery) {
  const params = new URLSearchParams()
  if (query.start) params.set('start', query.start)
  if (query.end) params.set('end', query.end)
  if (query.query) params.set('query', query.query)
  if (query.model) params.set('model', query.model)
  if (query.status && query.status !== 'all') params.set('status', query.status)
  return params
}

function usageStatisticsParams(query: UsageStatisticsQuery) {
  const params = new URLSearchParams()
  if (query.start) params.set('start', query.start)
  if (query.end) params.set('end', query.end)
  if (query.user_id != null) params.set('user_id', String(query.user_id))
  if (query.user_query) params.set('user_query', query.user_query)
  if (query.provider) params.set('provider', query.provider)
  if (query.model) params.set('model', query.model)
  if (query.billing_meter) params.set('billing_meter', query.billing_meter)
  if (query.page) params.set('page', String(query.page))
  if (query.limit) params.set('limit', String(query.limit))
  if (query.sort) params.set('sort', query.sort)
  return params
}
