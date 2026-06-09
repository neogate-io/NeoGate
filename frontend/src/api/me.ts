import type { LoginRole } from './auth'
import { ApiError, readApiErrorPayload } from '../utils/errors'

export type CurrentUser = {
  role: LoginRole
  requires_password_change: boolean
}

export async function getCurrentUser(token: string) {
  const response = await fetch('/api/me', {
    headers: {
      authorization: `Bearer ${token}`
    }
  })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) {
    const error = readApiErrorPayload(data)
    throw new ApiError(error?.message ?? response.statusText, response.status, error?.code)
  }

  const role = readRole(data)
  if (!role) {
    throw new ApiError('invalid current user response', response.status)
  }

  return { role, requires_password_change: readRequiresPasswordChange(data) }
}

function readRole(data: unknown): LoginRole | '' {
  if (typeof data !== 'object' || !data || !('role' in data)) return ''
  const role = (data as { role?: unknown }).role
  return role === 'admin' || role === 'user' ? role : ''
}

function readRequiresPasswordChange(data: unknown) {
  if (typeof data !== 'object' || !data || !('requires_password_change' in data)) return false
  return (data as { requires_password_change?: unknown }).requires_password_change === true
}
