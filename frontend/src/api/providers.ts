import type { ProviderRecord } from '../types/admin'
import { adminRequest } from './request'

export function getProviders() {
  return adminRequest<ProviderRecord[]>('/api/admin/providers')
}
