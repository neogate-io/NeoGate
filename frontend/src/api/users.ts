import type { User } from '../types/admin'
import { adminRequest } from './request'

export type UserPage = {
  items: User[]
  limit: number
  next_cursor?: string | null
  has_more?: boolean
}

export function getUsers(
  filters: { email?: string; apiKey?: string; limit?: number; cursor?: string } = {}
) {
  const searchParams = new URLSearchParams()
  if (filters.email) searchParams.set('email', filters.email)
  if (filters.apiKey) searchParams.set('api_key', filters.apiKey)
  if (filters.limit) searchParams.set('limit', String(filters.limit))
  if (filters.cursor) searchParams.set('cursor', filters.cursor)

  const query = searchParams.toString()
  return adminRequest<UserPage>(`/api/admin/users${query ? `?${query}` : ''}`)
}

export function updateUserStatus(id: number, status: User['status']) {
  return adminRequest<User>(`/api/admin/users/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({ status })
  })
}
