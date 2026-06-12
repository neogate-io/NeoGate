import type { CursorPage, User, UserStatus } from '../types/admin'
import { adminRequest } from './request'

export type UserPage = CursorPage<User>

export type GetUsersFilters = {
  search?: string
  email?: string
  apiKey?: string
  limit?: number
  cursor?: string
}

export type CreateUserPayload = {
  email: string
  username?: string | null
  password: string
  status?: UserStatus
}

export type UpdateUserPayload = {
  email?: string
  username?: string | null
  status?: UserStatus
  user_group_id?: number
}

export function getUsers(filters: GetUsersFilters = {}) {
  const searchParams = new URLSearchParams()
  if (filters.search) searchParams.set('search', filters.search)
  if (filters.email) searchParams.set('email', filters.email)
  if (filters.apiKey) searchParams.set('api_key', filters.apiKey)
  if (filters.limit) searchParams.set('limit', String(filters.limit))
  if (filters.cursor) searchParams.set('cursor', filters.cursor)

  const query = searchParams.toString()
  return adminRequest<UserPage>(`/api/admin/users${query ? `?${query}` : ''}`)
}

export function createUser(payload: CreateUserPayload) {
  return adminRequest<User>('/api/admin/users', {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function updateUser(id: number, payload: UpdateUserPayload) {
  return adminRequest<User>(`/api/admin/users/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(payload)
  })
}

export function updateUserStatus(id: number, status: UserStatus) {
  return updateUser(id, { status })
}

export function deleteUser(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/users/${id}`, {
    method: 'DELETE'
  })
}
