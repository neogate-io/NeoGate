import type { CursorPage, UsageRecord } from '../types/admin'
import { adminRequest, userRequest } from './request'

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
