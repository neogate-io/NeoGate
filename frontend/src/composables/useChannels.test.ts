import { reactive } from 'vue'
import { describe, expect, it } from 'vitest'
import { createChannelBaseUrlBinding } from './channelBaseUrl'
import type { ChannelForm } from './useChannels'

function channelForm(provider: string, openAiUrl = '', anthropicUrl = ''): ChannelForm {
  return reactive({
    provider,
    name: provider,
    models: '',
    enabled: true,
    use_credentials: false,
    secret: '',
    endpoints: {
      openai: { protocol: 'openai', base_url: openAiUrl, enabled: true },
      openai_oauth: { protocol: 'openai_oauth', base_url: '', enabled: true },
      anthropic: { protocol: 'anthropic', base_url: anthropicUrl, enabled: true }
    }
  })
}

describe('createChannelBaseUrlBinding', () => {
  it('keeps writing to the Anthropic endpoint after the field is cleared', () => {
    const form = channelForm('anthropic', '', 'https://api.anthropic.com')
    const binding = createChannelBaseUrlBinding(form)

    binding.value.value = ''
    binding.value.value = 'https://proxy.example.com'

    expect(binding.value.value).toBe('https://proxy.example.com')
    expect(form.endpoints.anthropic.base_url).toBe('https://proxy.example.com')
    expect(form.endpoints.openai.base_url).toBe('')
  })
})
