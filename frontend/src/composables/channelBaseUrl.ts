import { computed, ref } from 'vue'
import type { ChannelForm } from './useChannels'
import type { EndpointProtocol } from '../types/admin'

export function createChannelBaseUrlBinding(form: ChannelForm) {
  const protocol = ref(preferredBaseUrlProtocol(form))
  const value = computed({
    get: () => form.endpoints[protocol.value].base_url,
    set: (nextValue: string) => {
      form.endpoints[protocol.value].base_url = nextValue
    }
  })

  function syncProtocol() {
    protocol.value = preferredBaseUrlProtocol(form)
  }

  return { value, syncProtocol }
}

function preferredBaseUrlProtocol(form: ChannelForm): EndpointProtocol {
  if (form.provider === 'openai' && form.use_credentials) return 'openai_oauth'
  if (form.endpoints.openai.base_url.trim()) return 'openai'
  if (form.endpoints.anthropic.base_url.trim()) return 'anthropic'
  return form.provider === 'anthropic' ? 'anthropic' : 'openai'
}
