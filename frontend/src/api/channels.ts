import type {
  Channel,
  ChannelDiagnosticReport,
  ChannelKey,
  ChannelProvider,
  EndpointProtocol
} from '../types/admin'
import { adminRequest } from './request'

export type KeySelectionMode = 'polling' | 'random'

export function getChannels() {
  return adminRequest<Channel[]>('/api/admin/channels')
}

export function getChannelKeys(channelId: number) {
  return adminRequest<ChannelKey[]>(`/api/admin/channels/${channelId}/keys`)
}

export function getAllChannelKeys() {
  return adminRequest<ChannelKey[]>('/api/admin/channel-keys')
}

export function createChannelKey(channelId: number, payload: {
  name: string
  secret: string
  enabled: boolean
}) {
  return adminRequest<ChannelKey>(`/api/admin/channels/${channelId}/keys`, {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function fetchUpstreamModels(payload: {
  channel_id?: number
  provider: ChannelProvider
  protocol: EndpointProtocol
  base_url?: string
  secret?: string
  use_credentials?: boolean
}) {
  return adminRequest<{ models: string[] }>('/api/admin/upstream-models', {
    method: 'POST',
    body: JSON.stringify({
      channel_id: payload.channel_id,
      provider: payload.provider,
      protocol: payload.protocol,
      base_url: payload.base_url || undefined,
      secret: payload.secret || undefined,
      use_credentials: payload.use_credentials || undefined
    })
  })
}

export async function createChannel(payload: {
  provider: ChannelProvider
  name: string
  endpoints: Array<{
    protocol: EndpointProtocol
    base_url: string
    models: string[]
    enabled: boolean
  }>
  enabled: boolean
  priority: number
  weight: number
  key_selection_mode: KeySelectionMode
  use_credentials: boolean
  key_name?: string
  secret?: string
}) {
  const channel = await adminRequest<Channel>('/api/admin/channels', {
    method: 'POST',
    body: JSON.stringify({
      provider: payload.provider,
      name: payload.name,
      endpoints: payload.endpoints,
      enabled: payload.enabled,
      priority: payload.priority,
      weight: payload.weight,
      key_selection_mode: payload.key_selection_mode,
      use_credentials: payload.use_credentials
    })
  })

  if (!payload.use_credentials && payload.secret?.trim()) {
    await createChannelKey(channel.id, {
      name: payload.key_name?.trim() || payload.name,
      secret: payload.secret.trim(),
      enabled: true
    })
  }

  return channel
}

export function updateChannel(id: number, payload: {
  name: string
  endpoints: Array<{
    protocol: EndpointProtocol
    base_url: string
    models: string[]
    enabled: boolean
  }>
  enabled: boolean
  priority: number
  weight: number
  key_selection_mode: KeySelectionMode
  use_credentials: boolean
}) {
  return adminRequest<Channel>(`/api/admin/channels/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({
      name: payload.name,
      endpoints: payload.endpoints,
      enabled: payload.enabled,
      priority: payload.priority,
      weight: payload.weight,
      key_selection_mode: payload.key_selection_mode,
      use_credentials: payload.use_credentials
    })
  })
}

export function deleteChannel(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/channels/${id}`, {
    method: 'DELETE'
  })
}

export function diagnoseChannel(id: number) {
  return adminRequest<ChannelDiagnosticReport>(`/api/admin/channels/${id}/diagnose`, {
    method: 'POST'
  })
}
