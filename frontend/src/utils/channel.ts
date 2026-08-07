import type { ChannelProvider, EndpointProtocol, ProviderRecord } from '../types/admin'

export type ChannelProviderOption = {
  value: ChannelProvider
  label: string
  defaultName: string
  defaultEndpoints: Record<
    EndpointProtocol,
    {
      baseUrl: string
    }
  >
}

export const comfyUIOption: ChannelProviderOption = {
  value: 'comfyui',
  label: 'ComfyUI',
  defaultName: 'ComfyUI',
  defaultEndpoints: emptyDefaultEndpoints()
}

export function providerToOption(provider: ProviderRecord): ChannelProviderOption {
  if (isComfyUIProvider(provider.code)) {
    return comfyUIOption
  }

  const defaultEndpoints = emptyDefaultEndpoints()
  for (const endpoint of provider.default_endpoints) {
    defaultEndpoints[endpoint.protocol] = {
      baseUrl: endpoint.base_url
    }
  }

  return {
    value: provider.code,
    label: provider.display_name,
    defaultName: provider.display_name,
    defaultEndpoints
  }
}

export function sortProviderOptionsForDisplay(providers: ChannelProviderOption[]) {
  return moveComfyUiLast(providers, (provider) => provider.value)
}

export function sortProvidersForDisplay(providers: ProviderRecord[]) {
  return moveComfyUiLast(providers, (provider) => provider.code)
}

export function findProviderOption(provider: ChannelProvider, providers: ChannelProviderOption[]) {
  return providers.find((option) => option.value === provider)
}

export function formatModels(models: string[], allModelsLabel = '全部模型') {
  return models.length > 0 ? models.join(', ') : allModelsLabel
}

export function endpointLabel(protocol: EndpointProtocol) {
  if (protocol === 'openai_oauth') return 'OpenAI 令牌'
  return protocol === 'openai' ? 'OpenAI' : 'Anthropic'
}

function emptyDefaultEndpoints(): ChannelProviderOption['defaultEndpoints'] {
  return {
    openai: {
      baseUrl: ''
    },
    openai_oauth: {
      baseUrl: ''
    },
    anthropic: {
      baseUrl: ''
    }
  }
}

function moveComfyUiLast<T>(providers: T[], providerCode: (provider: T) => ChannelProvider) {
  return [
    ...providers.filter((provider) => !isComfyUIProvider(providerCode(provider))),
    ...providers.filter((provider) => isComfyUIProvider(providerCode(provider)))
  ]
}

function isComfyUIProvider(provider: ChannelProvider) {
  return provider.toLowerCase() === comfyUIOption.value
}

export function splitCommaList(value: string) {
  return Array.from(
    new Set(
      value
        .split(/[,，、;；\s]+/)
        .map((item) => item.trim())
        .filter(Boolean)
    )
  )
}
