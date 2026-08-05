import { describe, expect, it } from 'vitest'
import type { ProviderRecord } from '../types/admin'
import {
  comfyUIOption,
  providerToOption,
  sortProviderOptionsForDisplay,
  sortProvidersForDisplay,
  type ChannelProviderOption
} from './channel'

function providerOption(value: string): ChannelProviderOption {
  return {
    value,
    label: value,
    defaultName: value,
    defaultEndpoints: {
      openai: { baseUrl: '' },
      openai_oauth: { baseUrl: '' },
      anthropic: { baseUrl: '' }
    }
  }
}

function providerRecord(code: string): ProviderRecord {
  return {
    id: 1,
    code,
    display_name: code,
    name: code,
    default_endpoints: [],
    enabled: true,
    sort_order: 0
  }
}

describe('provider display order', () => {
  it('uses the dedicated ComfyUI option', () => {
    expect(providerToOption(providerRecord('comfyui'))).toBe(comfyUIOption)
  })

  it('puts ComfyUI last in the admin channel dialog', () => {
    const providers = ['openai', 'comfyui', 'anthropic'].map(providerOption)

    expect(sortProviderOptionsForDisplay(providers).map((provider) => provider.value)).toEqual([
      'openai',
      'anthropic',
      'comfyui'
    ])
  })

  it('puts ComfyUI last during initial setup', () => {
    const providers = ['openai', 'ComfyUI', 'anthropic'].map(providerRecord)

    expect(sortProvidersForDisplay(providers).map((provider) => provider.code)).toEqual([
      'openai',
      'anthropic',
      'ComfyUI'
    ])
  })
})
