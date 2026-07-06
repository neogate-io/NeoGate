import type { CursorPage, UserGroup, UserKey, UserKeyStatus } from '../types/admin'
import { adminRequest, publicRequest, userRequest } from './request'

export type UserKeyPage = CursorPage<UserKey>

export type GetUserKeysFilters = {
  userId?: number
  projectId?: number
  defaultProjectOnly?: boolean
  limit?: number
  cursor?: string
}

export type CreditAccountType = 'project' | 'user_key' | 'user_key_model'

export type AdjustCreditResponse = {
  balance_micros: number
}

export type CreatedUserKey = {
  record: UserKey
  key: string
}

export function getUserKeys(filters: GetUserKeysFilters = {}) {
  const searchParams = new URLSearchParams()
  if (filters.userId != null) searchParams.set('user_id', String(filters.userId))
  if (filters.projectId != null) searchParams.set('project_id', String(filters.projectId))
  if (filters.defaultProjectOnly) searchParams.set('default_project_only', 'true')
  if (filters.limit) searchParams.set('limit', String(filters.limit))
  if (filters.cursor) searchParams.set('cursor', filters.cursor)

  const query = searchParams.toString()
  return adminRequest<UserKeyPage>(`/api/admin/user-keys${query ? `?${query}` : ''}`)
}

export function getUserGroups() {
  return adminRequest<UserGroup[]>('/api/admin/user-groups')
}

export function getOwnUserKeys() {
  return userRequest<UserKey[]>('/api/user/apikeys')
}

export function createOwnUserKey(name: string) {
  return userRequest<{ record: UserKey; key: string }>('/api/user/apikeys', {
    method: 'POST',
    body: JSON.stringify({ name })
  })
}

export function deleteOwnUserKey(id: number) {
  return userRequest<{ ok: boolean }>(`/api/user/apikeys/${id}`, {
    method: 'DELETE'
  })
}

export function updateOwnUserKeyStatus(id: number, status: UserKeyStatus) {
  return userRequest<UserKey>(`/api/user/apikeys/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({ status })
  })
}

export function createUserKeyDraft() {
  return publicRequest<{ draft_id: string; masked_api_key: string }>('/api/user-key-drafts', {
    method: 'POST'
  })
}

export function createUserKey(email: string, draftId: string, locale: string) {
  return publicRequest<{ ok: boolean }>('/api/user-keys', {
    method: 'POST',
    body: JSON.stringify({ email, draft_id: draftId, locale })
  })
}

export function adjustCredit(
  creditAccountType: CreditAccountType,
  ownerId: number,
  amountMicros: number
) {
  return adminRequest<AdjustCreditResponse>('/api/admin/credits', {
    method: 'POST',
    body: JSON.stringify({
      credit_account_type: creditAccountType,
      owner_id: ownerId,
      amount_micros: amountMicros,
      reason: amountMicros > 0 ? 'recharge' : 'adjustment'
    })
  })
}

export function adjustDefaultProjectCredit(userId: number, amountMicros: number) {
  return adminRequest<AdjustCreditResponse>(`/api/admin/users/${userId}/default-project-credit`, {
    method: 'POST',
    body: JSON.stringify({
      amount_micros: amountMicros,
      reason: amountMicros > 0 ? 'recharge' : 'adjustment'
    })
  })
}

export function adjustUserKeyModelCredit(userKeyId: number, model: string, amountMicros: number) {
  return adminRequest<{
    user_key_model_id: number
    credit_account_id: number
    balance_micros: number
  }>('/api/admin/user-key-model-credits', {
    method: 'POST',
    body: JSON.stringify({
      user_key_id: userKeyId,
      model,
      amount_micros: amountMicros,
      reason: amountMicros > 0 ? 'recharge' : 'adjustment'
    })
  })
}
