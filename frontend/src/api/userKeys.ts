import type { UserGroup, UserKey } from '../types/admin'
import { adminRequest, publicRequest, userRequest } from './request'

export function getUserKeys(filters: { userId?: number } = {}) {
  const searchParams = new URLSearchParams()
  if (filters.userId != null) searchParams.set('user_id', String(filters.userId))

  const query = searchParams.toString()
  return adminRequest<UserKey[]>(`/api/admin/user-keys${query ? `?${query}` : ''}`)
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

export function updateOwnUserKeyStatus(id: number, status: UserKey['status']) {
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

export function adjustCredit(walletType: 'user' | 'user_key', walletId: number, amountMicroUsd: number) {
  return adminRequest<{ balance_micro_usd: number }>('/api/admin/credits', {
    method: 'POST',
    body: JSON.stringify({
      wallet_type: walletType,
      wallet_id: walletId,
      amount_micro_usd: amountMicroUsd,
      reason: amountMicroUsd > 0 ? 'recharge' : 'adjustment'
    })
  })
}
