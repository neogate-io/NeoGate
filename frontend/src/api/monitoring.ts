import type { UsageRecord } from '../types/admin'
import { adminRequest, userRequest } from './request'

export type UsagePage = {
  items: UsageRecord[]
  total: number
  page: number
  limit: number
}

export function getUsage(limit = 50) {
  return adminRequest<UsageRecord[]>(`/api/admin/usage?limit=${limit}`)
}

export function getUserUsage(page = 1, limit = 20, start?: string, end?: string) {
  const params = new URLSearchParams({
    page: String(page),
    limit: String(limit)
  })
  if (start) params.set('start', start)
  if (end) params.set('end', end)
  return userRequest<UsagePage>(`/api/user/usage?${params}`)
}
