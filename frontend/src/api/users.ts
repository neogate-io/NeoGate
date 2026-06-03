import type { User } from '../types/admin'
import { adminRequest } from './request'

export function getUsers(filters: { email?: string; apiKey?: string } = {}) {
  const searchParams = new URLSearchParams()
  if (filters.email) searchParams.set('email', filters.email)
  if (filters.apiKey) searchParams.set('api_key', filters.apiKey)

  const query = searchParams.toString()
  return adminRequest<User[]>(`/api/admin/users${query ? `?${query}` : ''}`)
}
