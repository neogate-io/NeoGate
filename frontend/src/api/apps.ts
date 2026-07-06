import type { AppRecord, AppRunLog, AppStatus, AppType } from '../types/admin'
import { adminRequest } from './request'

export type AppEndpointInput = {
  name?: string
  enabled?: boolean
  config?: Record<string, unknown>
  secrets?: Record<string, string>
}

export type AppModelOption = {
  model: string
  channel_count: number
}

export type CreateAppInput = {
  name: string
  description?: string
  app_type: AppType
  status?: AppStatus
  model: string
  system_prompt?: string
  context_turns?: number
  max_output_tokens?: number
  endpoint: AppEndpointInput
}

export type UpdateAppInput = Partial<Omit<CreateAppInput, 'app_type' | 'endpoint'>> & {
  endpoint?: AppEndpointInput
}

export function getApps(filters: { search?: string; status?: string; appType?: string } = {}) {
  const searchParams = new URLSearchParams()
  if (filters.search) searchParams.set('search', filters.search)
  if (filters.status) searchParams.set('status', filters.status)
  if (filters.appType) searchParams.set('app_type', filters.appType)
  const query = searchParams.toString()
  return adminRequest<AppRecord[]>(`/api/admin/apps${query ? `?${query}` : ''}`)
}

export function getAppModelOptions(filters: { userKeyId?: number } = {}) {
  const searchParams = new URLSearchParams()
  if (filters.userKeyId != null) searchParams.set('user_key_id', String(filters.userKeyId))
  const query = searchParams.toString()
  return adminRequest<AppModelOption[]>(`/api/admin/app-model-options${query ? `?${query}` : ''}`)
}

export function createApp(payload: CreateAppInput) {
  return adminRequest<AppRecord>('/api/admin/apps', {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function updateApp(id: number, payload: UpdateAppInput) {
  return adminRequest<AppRecord>(`/api/admin/apps/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(payload)
  })
}

export function deleteApp(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/apps/${id}`, {
    method: 'DELETE'
  })
}

export function testApp(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/apps/${id}/test`, {
    method: 'POST'
  })
}

export function getAppRunLogs(filters: {
  appId?: number
  endpointId?: number
  status?: string
  search?: string
  limit?: number
} = {}) {
  const searchParams = new URLSearchParams()
  if (filters.appId != null) searchParams.set('app_id', String(filters.appId))
  if (filters.endpointId != null) searchParams.set('endpoint_id', String(filters.endpointId))
  if (filters.status) searchParams.set('status', filters.status)
  if (filters.search) searchParams.set('search', filters.search)
  if (filters.limit) searchParams.set('limit', String(filters.limit))
  const query = searchParams.toString()
  return adminRequest<AppRunLog[]>(`/api/admin/app-run-logs${query ? `?${query}` : ''}`)
}
