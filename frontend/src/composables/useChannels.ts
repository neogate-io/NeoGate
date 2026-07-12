import { computed, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import {
  createChannel,
  createChannelKey,
  deleteChannel,
  deleteChannelKey,
  fetchUpstreamModels,
  getAllChannelKeys,
  getChannels,
  revealChannelKeySecret,
  updateChannel
} from '../api/channels'
import { getProviders } from '../api/providers'
import type { MessageKey } from '../i18n'
import { copyTextWithMessage } from '../utils/clipboard'
import { createConfirmAction } from '../utils/confirm'
import type {
  Channel,
  ChannelEndpoint,
  ChannelKey,
  ChannelProvider,
  EndpointProtocol
} from '../types/admin'
import {
  customProviderOption,
  findProviderOption,
  isManualBaseUrlProvider,
  providerToOption,
  splitCommaList,
  withCustomProviderLast,
  type ChannelProviderOption
} from '../utils/channel'
import { isNoModelsReturnedError, readError, readModelFetchError } from '../utils/errors'
import { withLoading, withLoadingValue } from './useLoadingTask'

type Translate = (key: MessageKey) => string
type ModelPickerTarget = {
  form: 'create' | 'edit'
}
type EndpointSubmit = {
  protocol: EndpointProtocol
  base_url: string
  models: string[]
  enabled: boolean
}

const protocols: EndpointProtocol[] = ['openai', 'openai_oauth', 'anthropic']
const wordJoiner = '\u2060'

export type ChannelEndpointForm = {
  protocol: EndpointProtocol
  base_url: string
  enabled: boolean
}

export type ChannelForm = {
  provider: Channel['provider']
  name: string
  models: string
  endpoints: Record<EndpointProtocol, ChannelEndpointForm>
  enabled: boolean
  use_credentials: boolean
  secret: string
}

function defaultEndpointForms(provider: ChannelProviderOption) {
  return {
    openai: {
      protocol: 'openai' as const,
      base_url: provider.defaultEndpoints.openai.baseUrl,
      enabled: true
    },
    openai_oauth: {
      protocol: 'openai_oauth' as const,
      base_url: provider.defaultEndpoints.openai_oauth.baseUrl,
      enabled: true
    },
    anthropic: {
      protocol: 'anthropic' as const,
      base_url: provider.defaultEndpoints.anthropic.baseUrl,
      enabled: true
    }
  }
}

function defaultCreateForm(provider: ChannelProviderOption = customProviderOption): ChannelForm {
  return {
    provider: provider.value,
    name: provider.defaultName,
    models: '',
    endpoints: defaultEndpointForms(provider),
    enabled: true,
    use_credentials: false,
    secret: ''
  }
}

function isValidHttpUrl(value: string) {
  try {
    const url = new URL(value)
    return url.protocol === 'http:' || url.protocol === 'https:'
  } catch {
    return false
  }
}

function endpointModels(endpoints: ChannelEndpoint[]) {
  const models = endpoints.flatMap((endpoint) => endpoint.models)
  return Array.from(new Set(models)).join(', ')
}

export function useChannels(t: Translate) {
  const confirmDialog = createConfirmAction(() => t('cancel'))
  const channels = ref<Channel[]>([])
  const channelKeys = ref<ChannelKey[]>([])
  const providers = ref<ChannelProviderOption[]>([])
  const loading = ref(true)
  const createDialogOpen = ref(false)
  const editDialogOpen = ref(false)
  const modelPickerDialogOpen = ref(false)
  const creating = ref(false)
  const fetchingModels = ref(false)
  const updating = ref(false)
  const deletingId = ref<number | null>(null)
  const deletingKeyId = ref<number | null>(null)
  const copyingKeyId = ref<number | null>(null)
  const editingChannel = ref<Channel | null>(null)
  const fetchedModels = ref<string[]>([])
  const selectedFetchedModels = ref<string[]>([])
  const modelPickerTarget = ref<ModelPickerTarget>({ form: 'create' })

  const createForm = reactive(defaultCreateForm())
  const editForm = reactive<ChannelForm>(defaultCreateForm())

  const createBaseUrl = computed({
    get: () => visibleBaseUrl(createForm),
    set: (value: string) => {
      setVisibleBaseUrl(createForm, value)
    }
  })

  const editBaseUrl = computed({
    get: () => visibleBaseUrl(editForm),
    set: (value: string) => {
      setVisibleBaseUrl(editForm, value)
    }
  })

  const secretInput = computed({
    get: () => keepHyphenWithNextChar(createForm.secret),
    set: (value: string) => {
      createForm.secret = stripWordJoiners(value)
    }
  })

  const editSecretInput = computed({
    get: () => keepHyphenWithNextChar(editForm.secret),
    set: (value: string) => {
      editForm.secret = stripWordJoiners(value)
    }
  })

  const providerOptions = computed(() => {
    return withCustomProviderLast(providers.value)
  })

  const keyCounts = computed(() => {
    const counts = new Map<number, number>()
    for (const key of channelKeys.value) {
      counts.set(key.channel_id, (counts.get(key.channel_id) ?? 0) + 1)
    }
    return counts
  })

  const editingChannelKeys = computed(() => {
    const channelId = editingChannel.value?.id
    if (!channelId) return []
    return channelKeys.value.filter((key) => key.channel_id === channelId)
  })

  const hasFetchedModels = computed(() => fetchedModels.value.length > 0)

  const allFetchedModelsSelected = computed(() => {
    return (
      hasFetchedModels.value && selectedFetchedModels.value.length === fetchedModels.value.length
    )
  })

  watch(selectedFetchedModels, syncSelectedModelsToInput, { deep: true })

  function openCreateDialog() {
    const provider = providerOptions.value[0]
    Object.assign(createForm, defaultCreateForm(provider))
    resetFetchedModels()
    createDialogOpen.value = true
  }

  function openEditDialog(row: Channel) {
    editingChannel.value = row
    const endpointByProtocol = new Map(
      row.endpoints.map((endpoint) => [endpoint.protocol, endpoint])
    )
    const provider = row.provider
    Object.assign(editForm, {
      provider,
      name: row.name,
      models: endpointModels(row.endpoints),
      endpoints: {
        openai: endpointFormFromRecord('openai', endpointByProtocol.get('openai')),
        openai_oauth: endpointFormFromRecord(
          'openai_oauth',
          endpointByProtocol.get('openai_oauth')
        ),
        anthropic: endpointFormFromRecord('anthropic', endpointByProtocol.get('anthropic'))
      },
      enabled: row.enabled,
      use_credentials: provider === 'openai' && row.use_credentials,
      secret: ''
    })
    resetFetchedModels()
    editDialogOpen.value = true
  }

  function endpointFormFromRecord(
    protocol: EndpointProtocol,
    endpoint?: ChannelEndpoint
  ): ChannelEndpointForm {
    return {
      protocol,
      base_url: endpoint?.base_url ?? '',
      enabled: endpoint?.enabled ?? true
    }
  }

  function selectCreateProvider(provider: ChannelProvider) {
    const option = findProviderOption(provider, providerOptions.value)
    if (!option) return

    Object.assign(createForm, defaultCreateForm(option))
    createForm.use_credentials = false
    resetFetchedModels()
  }

  async function loadChannels() {
    await withLoading(loading, async () => {
      try {
        const [fetchedChannels, fetchedChannelKeys, fetchedProviders] = await Promise.all([
          getChannels(),
          getAllChannelKeys(),
          getProviders()
        ])
        channels.value = fetchedChannels
        channelKeys.value = fetchedChannelKeys
        providers.value = fetchedProviders.map(providerToOption)
      } catch (err) {
        channels.value = []
        channelKeys.value = []
        ElMessage.error(readError(err))
      }
    })
  }

  async function submitChannel(beforeCreate?: () => Promise<boolean>) {
    const parsed = validateChannelForm(createForm)
    if (!parsed) return null
    const secrets = createForm.use_credentials ? [] : splitSecretLines(createForm.secret)

    return withLoading(creating, async () => {
      try {
        if (beforeCreate && !(await beforeCreate())) {
          return null
        }
        const channel = await createChannel({
          provider: createForm.provider,
          name: parsed.name,
          endpoints: parsed.endpoints,
          enabled: createForm.enabled,
          priority: 0,
          weight: 1,
          key_selection_mode: 'polling',
          use_credentials: createForm.use_credentials
        })
        await createKeysFromSecrets(channel.id, parsed.name, secrets)
        ElMessage.success(t('channelCreated'))
        await loadChannels()
        createDialogOpen.value = false
        return channel
      } catch (err) {
        ElMessage.error(readError(err))
        return null
      }
    })
  }

  async function fetchCreateModels() {
    await fetchModels('create')
  }

  async function fetchEditModels() {
    await fetchModels('edit')
  }

  async function fetchModels(formTarget: ModelPickerTarget['form']) {
    const form = formTarget === 'edit' ? editForm : createForm
    const channelId = formTarget === 'edit' ? editingChannel.value?.id : undefined
    if (formTarget === 'edit' && !channelId) return

    const endpoint = modelFetchEndpoint(form)
    const baseUrl = endpoint.base_url.trim()
    const secret =
      formTarget === 'create'
        ? (splitSecretLines(createForm.secret)[0] ?? '')
        : splitSecretLines(editForm.secret)[0]
    if (!validateModelFetchInput(form, baseUrl, secret)) return

    modelPickerTarget.value = { form: formTarget }
    const shouldKeepAllSelected = allFetchedModelsSelected.value
    const existingModels = splitCommaList(form.models)

    await withLoading(fetchingModels, async () => {
      try {
        const { models } = await fetchUpstreamModels({
          channel_id: channelId,
          provider: form.provider,
          protocol: endpoint.protocol,
          base_url: baseUrl,
          secret,
          use_credentials: form.use_credentials
        })

        const selectableModels = mergeModelLists(models, existingModels)
        fetchedModels.value = selectableModels
        selectedFetchedModels.value =
          shouldKeepAllSelected ? selectableModels : existingModels
        syncSelectedModelsToInput()
        modelPickerDialogOpen.value = true
        if (models.length === 0) {
          ElMessage.warning(t('modelsFetchEmpty'))
        } else {
          ElMessage.success(t('modelsFetched'))
        }
      } catch (err) {
        if (isNoModelsReturnedError(err)) {
          fetchedModels.value = existingModels
          selectedFetchedModels.value = existingModels
          modelPickerDialogOpen.value = true
          ElMessage.warning(t('modelsFetchEmpty'))
          return
        }
        ElMessage.error(readModelFetchError(err, t))
      }
    })
  }

  function mergeModelLists(primary: string[], secondary: string[]) {
    return Array.from(new Set([...primary, ...secondary]))
  }

  function validateModelFetchInput(form: ChannelForm, baseUrl: string, secret?: string) {
    if (form.use_credentials && !supportsCredentialFiles(form)) {
      ElMessage.warning(t('credentialFilesUnsupportedProvider'))
      return false
    }

    if (!form.use_credentials && secret === '') {
      ElMessage.warning(t('upstreamKeyRequired'))
      return false
    }

    if (!baseUrl) {
      ElMessage.warning(t('baseUrlRequired'))
      return false
    }

    if (!isValidHttpUrl(baseUrl)) {
      ElMessage.warning(t('baseUrlInvalid'))
      return false
    }

    return true
  }

  async function submitEditChannel() {
    if (!editingChannel.value) return null

    const parsed = validateChannelForm(editForm)
    if (!parsed) return null

    const editing = editingChannel.value
    return withLoading(updating, async () => {
      try {
        const channel = await updateChannel(editing.id, {
          name: parsed.name,
          endpoints: parsed.endpoints,
          enabled: editForm.enabled,
          priority: editing.priority,
          weight: editing.weight,
          key_selection_mode: editing.key_selection_mode,
          use_credentials: editForm.use_credentials
        })
        const secrets = editForm.use_credentials ? [] : splitSecretLines(editForm.secret)
        await createKeysFromSecrets(editing.id, parsed.name, secrets)
        ElMessage.success(t('channelUpdated'))
        await loadChannels()
        editDialogOpen.value = false
        return channel
      } catch (err) {
        ElMessage.error(readError(err))
        return null
      }
    })
  }

  async function confirmDeleteChannelKey(key: ChannelKey) {
    const confirmed = await confirmDialog(t('deleteChannelKeyConfirm'), t('delete'), {
      confirmText: t('delete'),
      danger: true,
      type: 'warning'
    })
    if (!confirmed) return

    await withLoadingValue(deletingKeyId, key.id, null, async () => {
      try {
        await deleteChannelKey(key.channel_id, key.id)
        ElMessage.success(t('channelKeyDeleted'))
        channelKeys.value = channelKeys.value.filter((item) => item.id !== key.id)
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
  }

  async function copyChannelKeySecret(key: ChannelKey) {
    if (copyingKeyId.value) return

    await withLoadingValue(copyingKeyId, key.id, null, async () => {
      try {
        const { secret } = await revealChannelKeySecret(key.channel_id, key.id)
        await copyTextWithMessage(secret, t('channelKeyCopied'))
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
  }

  function toggleAllFetchedModels(checked: boolean) {
    selectedFetchedModels.value = checked ? [...fetchedModels.value] : []
    syncSelectedModelsToInput()
  }

  function resetFetchedModels() {
    modelPickerTarget.value = { form: 'create' }
    fetchedModels.value = []
    selectedFetchedModels.value = []
    modelPickerDialogOpen.value = false
  }

  function syncSelectedModelsToInput() {
    if (!hasFetchedModels.value) return
    const models = selectedFetchedModels.value.join(', ')
    const form = modelPickerTarget.value.form === 'edit' ? editForm : createForm
    form.models = models
  }

  function modelsInputPlaceholder() {
    if (allFetchedModelsSelected.value) return t('allModels')
    return hasFetchedModels.value ? t('modelsCommaSeparated') : t('modelsFetchRequired')
  }

  function modelsInputReadonly() {
    return true
  }

  function validateChannelForm(form: ChannelForm) {
    const name = form.name.trim()
    if (!name) {
      ElMessage.warning(t('channelNameRequired'))
      return null
    }

    if (form.use_credentials && !supportsCredentialFiles(form)) {
      ElMessage.warning(t('credentialFilesUnsupportedProvider'))
      return null
    }

    const models = splitCommaList(form.models)
    if (models.length === 0) {
      ElMessage.warning(t('channelModelsRequired'))
      return null
    }

    const endpoints = channelEndpointsForSubmit(form, models)
    if (!endpoints) return null

    return { name, endpoints }
  }

  function channelEndpointsForSubmit(form: ChannelForm, models: string[]) {
    if (form.provider === 'openai' && form.use_credentials) {
      const baseUrl = form.endpoints.openai_oauth.base_url.trim()
      if (!baseUrl) {
        ElMessage.warning(t('baseUrlRequired'))
        return null
      }

      if (!isValidHttpUrl(baseUrl)) {
        ElMessage.warning(t('baseUrlInvalid'))
        return null
      }

      return [
        {
          protocol: 'openai_oauth' as const,
          base_url: baseUrl,
          models,
          enabled: true
        }
      ]
    }

    if (isManualBaseUrlProvider(form.provider)) {
      const endpoints = manualProviderEndpointsForSubmit(form, models)
      if (!endpoints) return null
      if (endpoints.length === 0) {
        ElMessage.warning(t('baseUrlRequired'))
        return null
      }

      return endpoints
    }

    const endpoints: EndpointSubmit[] = []
    for (const protocol of protocols) {
      if (protocol === 'openai_oauth') continue
      const endpoint = form.endpoints[protocol]
      const baseUrl = endpoint.base_url.trim()
      if (!baseUrl) continue

      if (!isValidHttpUrl(baseUrl)) {
        ElMessage.warning(t('baseUrlInvalid'))
        return null
      }

      endpoints.push({
        protocol,
        base_url: baseUrl,
        models,
        enabled: endpoint.enabled
      })
    }

    if (endpoints.length === 0) {
      ElMessage.warning(t('channelModelsRequired'))
      return null
    }

    return endpoints
  }

  function stripWordJoiners(value: string) {
    return value.replace(/\u2060/g, '')
  }

  function splitSecretLines(value: string) {
    return stripWordJoiners(value)
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean)
  }

  async function createKeysFromSecrets(channelId: number, name: string, secrets: string[]) {
    for (const [index, secret] of secrets.entries()) {
      await createChannelKey(channelId, {
        name: secrets.length > 1 ? `${name} ${index + 1}` : name,
        secret,
        enabled: true
      })
    }
  }

  function keepHyphenWithNextChar(value: string) {
    return stripWordJoiners(value).replace(/-/g, `-${wordJoiner}`)
  }

  function modelFetchEndpoint(form: ChannelForm) {
    if (form.provider === 'openai' && form.use_credentials) {
      return form.endpoints.openai_oauth
    }

    if (isManualBaseUrlProvider(form.provider)) {
      return form.endpoints.openai.base_url.trim()
        ? form.endpoints.openai
        : form.endpoints.anthropic
    }

    if (form.endpoints.openai.base_url.trim()) {
      return form.endpoints.openai
    }

    return (
      protocols
        .map((protocol) => form.endpoints[protocol])
        .find((endpoint) => endpoint.base_url.trim()) ?? form.endpoints.openai
    )
  }

  function supportsCredentialFiles(form: ChannelForm) {
    return form.provider === 'openai'
  }

  function visibleBaseUrl(form: ChannelForm) {
    if (form.provider === 'openai' && form.use_credentials) {
      return form.endpoints.openai_oauth.base_url
    }

    return form.endpoints.openai.base_url || form.endpoints.anthropic.base_url
  }

  function setVisibleBaseUrl(form: ChannelForm, value: string) {
    modelFetchEndpoint(form).base_url = value
  }

  function manualProviderEndpointsForSubmit(form: ChannelForm, models: string[]) {
    const endpoints: EndpointSubmit[] = []

    for (const protocol of ['openai', 'anthropic'] as const) {
      const baseUrl = form.endpoints[protocol].base_url.trim()
      if (!baseUrl) continue

      if (!isValidHttpUrl(baseUrl)) {
        ElMessage.warning(t('baseUrlInvalid'))
        return null
      }

      endpoints.push({
        protocol,
        base_url: baseUrl,
        models,
        enabled: true
      })
    }

    return endpoints
  }

  watch(
    () => createForm.use_credentials,
    (useCredentials) => {
      syncOpenAiCredentialEndpoint(createForm, useCredentials)
      resetFetchedModels()
    }
  )

  watch(
    () => editForm.use_credentials,
    (useCredentials) => {
      syncOpenAiCredentialEndpoint(editForm, useCredentials)
      resetFetchedModels()
    }
  )

  function syncOpenAiCredentialEndpoint(form: ChannelForm, useCredentials: boolean) {
    if (form.provider !== 'openai') return
    if (useCredentials) {
      if (!form.endpoints.openai_oauth.base_url) {
        const provider = findProviderOption(form.provider, providerOptions.value)
        form.endpoints.openai_oauth.base_url = provider?.defaultEndpoints.openai_oauth.baseUrl ?? ''
      }
      return
    }
    if (!form.endpoints.openai.base_url) {
      const provider = findProviderOption(form.provider, providerOptions.value)
      form.endpoints.openai.base_url = provider?.defaultEndpoints.openai.baseUrl ?? ''
    }
  }

  async function confirmDeleteChannel(row: Channel) {
    const confirmed = await confirmDialog(t('deleteChannelConfirm'), t('delete'), {
      confirmText: t('delete'),
      danger: true,
      type: 'warning'
    })
    if (!confirmed) return

    await withLoadingValue(deletingId, row.id, null, async () => {
      try {
        await deleteChannel(row.id)
        ElMessage.success(t('channelDeleted'))
        await loadChannels()
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
  }

  return {
    channels,
    channelKeys,
    providerOptions,
    protocols,
    keyCounts,
    loading,
    createDialogOpen,
    editDialogOpen,
    modelPickerDialogOpen,
    creating,
    fetchingModels,
    updating,
    deletingId,
    deletingKeyId,
    copyingKeyId,
    editingChannel,
    editingChannelKeys,
    createForm,
    editForm,
    createBaseUrl,
    editBaseUrl,
    secretInput,
    editSecretInput,
    fetchedModels,
    selectedFetchedModels,
    allFetchedModelsSelected,
    hasFetchedModels,
    modelsInputPlaceholder,
    modelsInputReadonly,
    selectCreateProvider,
    openCreateDialog,
    openEditDialog,
    fetchCreateModels,
    fetchEditModels,
    toggleAllFetchedModels,
    loadChannels,
    submitChannel,
    submitEditChannel,
    confirmDeleteChannelKey,
    copyChannelKeySecret,
    confirmDeleteChannel
  }
}
