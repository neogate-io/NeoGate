import type { ChannelProvider, EndpointProtocol, ProviderRecord } from '../types/admin'

export type ChannelProviderOption = {
  value: ChannelProvider
  label: string
  defaultName: string
  defaultModels: string
  defaultEndpoints: Record<EndpointProtocol, {
    baseUrl: string
  }>
}

export const customProviderOption: ChannelProviderOption = {
  value: 'custom',
  label: '自定义',
  defaultName: '',
  defaultModels: '',
  defaultEndpoints: emptyDefaultEndpoints()
}

export const newapiProviderOption: ChannelProviderOption = {
  value: 'newapi',
  label: 'NewAPI',
  defaultName: '',
  defaultModels: '',
  defaultEndpoints: emptyDefaultEndpoints()
}

export const sub2apiProviderOption: ChannelProviderOption = {
  value: 'sub2api',
  label: 'Sub2API',
  defaultName: '',
  defaultModels: '',
  defaultEndpoints: emptyDefaultEndpoints()
}

export function providerToOption(provider: ProviderRecord): ChannelProviderOption {
  if (isCustomProvider(provider.code)) {
    return customProviderOption
  }

  if (isNewapiProvider(provider.code)) {
    return newapiProviderOption
  }

  if (isSub2apiProvider(provider.code)) {
    return sub2apiProviderOption
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
    defaultModels: provider.default_models.join(','),
    defaultEndpoints
  }
}

export function isCustomProvider(provider: ChannelProvider) {
  return provider === customProviderOption.value
}

export function isNewapiProvider(provider: ChannelProvider) {
  return provider === newapiProviderOption.value
}

export function isSub2apiProvider(provider: ChannelProvider) {
  return provider === sub2apiProviderOption.value
}

export function isManualBaseUrlProvider(provider: ChannelProvider) {
  return isCustomProvider(provider) || isNewapiProvider(provider) || isSub2apiProvider(provider)
}

export function withCustomProviderLast(providers: ChannelProviderOption[]) {
  return [
    ...providers.filter((provider) => !isManualBaseUrlProvider(provider.value)),
    newapiProviderOption,
    sub2apiProviderOption,
    customProviderOption
  ]
}

export function sortProvidersForDisplay(providers: ProviderRecord[]) {
  const custom = providers.filter((provider) => isCustomProvider(provider.code))
  const rest = providers.filter((provider) => !isCustomProvider(provider.code))
  return [...rest, ...custom]
}

export function findProviderOption(
  provider: ChannelProvider,
  providers: ChannelProviderOption[]
) {
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
