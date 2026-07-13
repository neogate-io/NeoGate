import type {
  Channel,
  ChannelDiagnosticReport,
  ChannelKey,
  ChannelModel,
  ChannelProvider,
  DiagnosticStep,
  EndpointProtocol
} from '../types/admin'
import { adminRequest } from './request'
import { useAuthStore } from '../stores/auth'
import { ApiError } from '../utils/errors'

export type KeySelectionMode = 'polling' | 'random'
export type ChannelDiagnosticScope = 'all' | 'models' | 'text' | 'image' | 'video'

export function getChannels() {
  return adminRequest<Channel[]>('/api/admin/channels')
}

export function getChannelKeys(channelId: number) {
  return adminRequest<ChannelKey[]>(`/api/admin/channels/${channelId}/keys`)
}

export function getAllChannelKeys() {
  return adminRequest<ChannelKey[]>('/api/admin/channel-keys')
}

export function createChannelKey(
  channelId: number,
  payload: {
    name: string
    secret: string
    enabled: boolean
  }
) {
  return adminRequest<ChannelKey>(`/api/admin/channels/${channelId}/keys`, {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function deleteChannelKey(channelId: number, keyId: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/channels/${channelId}/keys/${keyId}`, {
    method: 'DELETE'
  })
}

export function revealChannelKeySecret(channelId: number, keyId: number) {
  return adminRequest<{ secret: string }>(`/api/admin/channels/${channelId}/keys/${keyId}/secret`)
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

export function updateChannel(
  id: number,
  payload: {
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
  }
) {
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

export function updateChannelModel(
  channelId: number,
  model: string,
  payload: { enabled: boolean }
) {
  return adminRequest<ChannelModel>(
    `/api/admin/channels/${channelId}/models/${encodeURIComponent(model)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(payload)
    }
  )
}

export function deleteChannel(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/channels/${id}`, {
    method: 'DELETE'
  })
}

export function diagnoseChannel(id: number, scope: ChannelDiagnosticScope = 'all') {
  return adminRequest<ChannelDiagnosticReport>(`/api/admin/channels/${id}/diagnose`, {
    method: 'POST',
    body: JSON.stringify({ scope })
  })
}

export type ChannelDiagnosticStreamEvent =
  | {
      type: 'started'
      channel_id: number
      channel_name: string
      provider: string
    }
  | {
      type: 'model_started'
      endpoint_id: number
      protocol: string
      base_url: string
      key_id?: number | null
      key_name: string
      key_prefix?: string | null
      model: string
    }
  | {
      type: 'model_result'
      endpoint_id: number
      protocol: string
      base_url: string
      key_id?: number | null
      key_name: string
      key_prefix?: string | null
      model: string
      step: DiagnosticStep
    }
  | {
      type: 'finished'
      report: ChannelDiagnosticReport
    }
  | {
      type: 'error'
      message: string
    }

export async function streamChannelDiagnostic(
  id: number,
  scope: ChannelDiagnosticScope,
  onEvent: (event: ChannelDiagnosticStreamEvent) => void
) {
  const auth = useAuthStore()
  const headers = new Headers()
  if (auth.token) headers.set('authorization', `Bearer ${auth.token}`)
  headers.set('content-type', 'application/json')

  const response = await fetch(`/api/admin/channels/${id}/diagnose/stream`, {
    method: 'POST',
    headers,
    body: JSON.stringify({ scope })
  })
  if (!response.ok || !response.body) {
    throw new ApiError(response.statusText, response.status)
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let report: ChannelDiagnosticReport | null = null

  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const chunks = buffer.split('\n\n')
    buffer = chunks.pop() ?? ''
    for (const chunk of chunks) {
      const data = chunk
        .split('\n')
        .filter((line) => line.startsWith('data:'))
        .map((line) => line.slice(5).trimStart())
        .join('\n')
      if (!data) continue
      const event = JSON.parse(data) as ChannelDiagnosticStreamEvent
      onEvent(event)
      if (event.type === 'finished') report = event.report
      if (event.type === 'error') throw new Error(event.message)
    }
  }

  if (!report) throw new Error('Diagnostic stream ended without a final report')
  return report
}
