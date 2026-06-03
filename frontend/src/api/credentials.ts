import type { Credential, CredentialUploadResult } from '../types/admin'
import { adminRequest, adminUploadRequest } from './request'

export function getCredentials(provider?: string) {
  const query = provider ? `?provider=${encodeURIComponent(provider)}` : ''
  return adminRequest<Credential[]>(`/api/admin/credentials${query}`)
}

export function uploadCredentialFile(file: File) {
  const form = new FormData()
  form.append('file', file)
  return adminUploadRequest<CredentialUploadResult>('/api/admin/credentials/upload', form)
}

export function refreshCredential(id: number) {
  return adminRequest<Credential>(`/api/admin/credentials/${id}/refresh`, {
    method: 'POST'
  })
}

export function enableCredential(id: number) {
  return adminRequest<Credential>(`/api/admin/credentials/${id}/enable`, {
    method: 'POST'
  })
}

export function disableCredential(id: number) {
  return adminRequest<Credential>(`/api/admin/credentials/${id}/disable`, {
    method: 'POST'
  })
}

export function deleteCredential(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/credentials/${id}`, {
    method: 'DELETE'
  })
}
