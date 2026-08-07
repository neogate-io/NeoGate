import { computed, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import {
  createApp,
  getAppModelOptions,
  type AppModelOption,
  type CreateAppInput,
  type UpdateAppInput
} from '../api/apps'
import type { MessageKey } from '../i18n'
import type { AppRecord, AppType } from '../types/admin'
import { readError } from '../utils/errors'
import { useLatestTask } from './useLatestTask'
import { withLoading } from './useLoadingTask'
import { useLocale } from './useLocale'

export const appTypes = [
  {
    type: 'wecom',
    labelKey: 'appTypeWecom',
    descriptionKey: 'appTypeWecomDescription',
    iconUrl: '/icons/wecom.svg',
    enabled: true
  },
  {
    type: 'feishu',
    labelKey: 'appTypeFeishu',
    descriptionKey: 'appTypeFeishuDescription',
    iconUrl: '/icons/feishu.svg',
    enabled: true
  },
  {
    type: 'dingtalk',
    labelKey: 'appTypeDingtalk',
    descriptionKey: 'appTypeDingtalkDescription',
    iconUrl: '/icons/dingtalk.svg',
    enabled: true
  },
  {
    type: 'webhook',
    labelKey: 'appTypeWebhook',
    descriptionKey: 'appTypeWebhookDescription',
    iconUrl: '/icons/webhook.svg',
    enabled: true
  },
  {
    type: 'widget',
    labelKey: 'appTypeWidget',
    descriptionKey: 'appTypeWidgetDescription',
    iconUrl: '/icons/widget.svg',
    enabled: true
  }
] as const

export const usageScenarios = [
  {
    value: 'brief_qa',
    labelKey: 'appScenarioBriefQa',
    descriptionKey: 'appScenarioBriefQaDescription',
    contextTurns: 4,
    maxOutputTokens: 1024,
    promptKey: 'appScenarioBriefQaPrompt'
  },
  {
    value: 'customer_support',
    labelKey: 'appScenarioCustomerSupport',
    descriptionKey: 'appScenarioCustomerSupportDescription',
    contextTurns: 8,
    maxOutputTokens: 1536,
    promptKey: 'appScenarioCustomerSupportPrompt'
  },
  {
    value: 'knowledge',
    labelKey: 'appScenarioKnowledge',
    descriptionKey: 'appScenarioKnowledgeDescription',
    contextTurns: 10,
    maxOutputTokens: 2048,
    promptKey: 'appScenarioKnowledgePrompt'
  },
  {
    value: 'analysis',
    labelKey: 'appScenarioAnalysis',
    descriptionKey: 'appScenarioAnalysisDescription',
    contextTurns: 16,
    maxOutputTokens: 4096,
    promptKey: 'appScenarioAnalysisPrompt'
  },
  {
    value: 'notification',
    labelKey: 'appScenarioNotification',
    descriptionKey: 'appScenarioNotificationDescription',
    contextTurns: 2,
    maxOutputTokens: 512,
    promptKey: 'appScenarioNotificationPrompt'
  }
] as const

export type UsageScenario = (typeof usageScenarios)[number]['value']

export function typeMeta(type: string) {
  return appTypes.find((item) => item.type === type) ?? appTypes[0]
}

export function useAppCreate() {
  const { t } = useLocale()
  const modelOptions = ref<AppModelOption[]>([])
  const saving = ref(false)
  const createdApp = ref<AppRecord | null>(null)
  const lastAutoSystemPrompt = ref('')
  const lastAutoModel = ref('')
  const modelOptionsTask = useLatestTask()

  function typeLabel(type: string) {
    return t(typeMeta(type).labelKey as MessageKey)
  }

  const form = reactive({
    step: 1,
    appType: 'wecom' as AppType,
    name: '',
    description: '',
    status: 'enabled',
    model: '',
    usageScenario: 'brief_qa' as UsageScenario,
    systemPrompt: '',
    contextTurns: 10,
    maxOutputTokens: 2048,
    endpointEnabled: true,
    corpId: '',
    agentId: '',
    corpSecret: '',
    callbackToken: '',
    encodingAesKey: '',
    feishuAppId: '',
    feishuAppSecret: '',
    feishuVerificationToken: '',
    feishuEncryptKey: '',
    dingtalkAppSecret: '',
    webhookSecret: '',
    allowedDomains: '',
    welcome: '',
    themeColor: '#176baf',
    anonymousAccess: true
  })

  const createDialogTitle = computed(() => {
    if (form.step === 3) {
      return t('appAccessInfoTitle', {
        type: typeLabel(createdApp.value?.app_type ?? form.appType)
      })
    }
    return form.step === 2 ? t('appCreateTitle', { type: typeLabel(form.appType) }) : t('appNew')
  })
  const selectedUsageScenario = computed(
    () => usageScenarios.find((item) => item.value === form.usageScenario) ?? usageScenarios[2]
  )
  const canApplyScenarioPrompt = computed(
    () => form.systemPrompt !== t(selectedUsageScenario.value.promptKey as MessageKey)
  )
  const createdEndpoint = computed(() => createdApp.value?.endpoint ?? null)
  const createdAccessUrls = computed(() => {
    const endpoint = createdEndpoint.value
    if (!endpoint) return []
    if (endpoint.endpoint_type === 'wecom') {
      return [
        {
          label: t('appReceiveMessageUrl'),
          value: endpoint.callback_url,
          helper: t('appReceiveMessageUrlHelp')
        }
      ]
    }
    if (endpoint.endpoint_type === 'feishu') {
      return [
        {
          label: t('appEventSubscriptionUrl'),
          value: endpoint.callback_url,
          helper: t('appEventSubscriptionUrlHelp')
        }
      ]
    }
    if (endpoint.endpoint_type === 'dingtalk') {
      return [
        {
          label: t('appBotMessageUrl'),
          value: endpoint.callback_url,
          helper: t('appBotMessageUrlHelp')
        }
      ]
    }
    if (endpoint.endpoint_type === 'webhook') {
      return [
        {
          label: 'Webhook URL',
          value: endpoint.invoke_url,
          helper: t('appWebhookUrlHelp')
        }
      ]
    }
    if (endpoint.endpoint_type === 'widget') {
      return [
        {
          label: t('appEmbedScript'),
          value: endpoint.widget_script_url,
          helper: t('appEmbedScriptHelp')
        },
        {
          label: t('appMessageEndpoint'),
          value: endpoint.invoke_url,
          helper: t('appMessageEndpointHelp')
        }
      ]
    }
    return []
  })

  function applyUsageScenario(
    value: string | number | boolean,
    options: { forcePrompt?: boolean } = {}
  ) {
    const scenario = usageScenarios.find((item) => item.value === value)
    if (!scenario) return
    const prompt = t(scenario.promptKey as MessageKey)
    form.contextTurns = scenario.contextTurns
    form.maxOutputTokens = scenario.maxOutputTokens
    if (
      options.forcePrompt ||
      !form.systemPrompt.trim() ||
      form.systemPrompt === lastAutoSystemPrompt.value
    ) {
      form.systemPrompt = prompt
      lastAutoSystemPrompt.value = prompt
    }
    applyRecommendedModel(options.forcePrompt)
  }

  function applySelectedScenarioPrompt() {
    const prompt = t(selectedUsageScenario.value.promptKey as MessageKey)
    form.systemPrompt = prompt
    lastAutoSystemPrompt.value = prompt
  }

  function recommendedModelForScenario() {
    const candidates = modelOptions.value.map((item) => item.model)
    if (candidates.length === 0) return ''
    const preferred: Record<UsageScenario, string[]> = {
      brief_qa: ['mini', 'flash', 'turbo', 'lite'],
      customer_support: ['plus', 'mini', 'flash', 'turbo'],
      knowledge: ['plus', 'mini', 'pro'],
      analysis: ['reasoner', 'reasoning', 'pro', 'max', 'gpt-4.1'],
      notification: ['mini', 'flash', 'turbo', 'lite']
    }
    const keywords = preferred[form.usageScenario]
    return (
      keywords
        .map((keyword) =>
          candidates.find((model) =>
            model.toLocaleLowerCase().includes(keyword.toLocaleLowerCase())
          )
        )
        .find(Boolean) ?? candidates[0]
    )
  }

  function applyRecommendedModel(force = false) {
    if (!force && form.model && form.model !== lastAutoModel.value) return
    const model = recommendedModelForScenario()
    if (!model) return
    form.model = model
    lastAutoModel.value = model
  }

  function resetForm(type: AppType = 'wecom') {
    form.step = 1
    createdApp.value = null
    form.appType = type
    form.name = ''
    form.description = ''
    form.status = 'enabled'
    form.model = ''
    lastAutoModel.value = ''
    form.usageScenario = 'brief_qa'
    form.systemPrompt = ''
    form.contextTurns = 10
    form.maxOutputTokens = 2048
    form.endpointEnabled = true
    lastAutoSystemPrompt.value = ''
    applyUsageScenario(form.usageScenario, { forcePrompt: true })
    form.corpId = ''
    form.agentId = ''
    form.corpSecret = ''
    form.callbackToken = ''
    form.encodingAesKey = ''
    form.feishuAppId = ''
    form.feishuAppSecret = ''
    form.feishuVerificationToken = ''
    form.feishuEncryptKey = ''
    form.dingtalkAppSecret = ''
    form.webhookSecret = ''
    form.allowedDomains = ''
    form.welcome = ''
    form.themeColor = '#176baf'
    form.anonymousAccess = true
  }

  function selectType(type: AppType, enabled: boolean) {
    if (!enabled) {
      ElMessage.info(t('appTypeComingSoon'))
      return
    }
    form.appType = type
    form.step = 2
  }

  function endpointConfig() {
    if (form.appType === 'wecom') {
      return {
        corp_id: form.corpId,
        agent_id: form.agentId
      }
    }
    if (form.appType === 'feishu') {
      return {
        app_id: form.feishuAppId
      }
    }
    if (form.appType === 'widget') {
      return {
        allowed_domains: form.allowedDomains
          .split('\n')
          .map((item) => item.trim())
          .filter(Boolean),
        welcome: form.welcome,
        theme_color: form.themeColor,
        anonymous_access: form.anonymousAccess
      }
    }
    return {}
  }

  function endpointSecrets(): Record<string, string> {
    if (form.appType === 'wecom') {
      return {
        corp_secret: form.corpSecret,
        token: form.callbackToken,
        aes_key: form.encodingAesKey
      }
    }
    if (form.appType === 'webhook') {
      return { secret: form.webhookSecret }
    }
    if (form.appType === 'feishu') {
      return {
        app_secret: form.feishuAppSecret,
        verification_token: form.feishuVerificationToken,
        encrypt_key: form.feishuEncryptKey
      }
    }
    if (form.appType === 'dingtalk') {
      return { app_secret: form.dingtalkAppSecret }
    }
    return {}
  }

  function payload(): CreateAppInput {
    return {
      name: form.name,
      description: form.description,
      app_type: form.appType,
      status: form.status as 'enabled' | 'disabled',
      model: form.model,
      system_prompt: form.systemPrompt,
      context_turns: form.contextTurns,
      max_output_tokens: form.maxOutputTokens,
      endpoint: {
        name: form.name,
        enabled: form.status === 'enabled',
        config: endpointConfig(),
        secrets: endpointSecrets()
      }
    }
  }

  function updatePayload(): UpdateAppInput {
    return {
      name: form.name,
      description: form.description,
      status: form.status as 'enabled' | 'disabled',
      model: form.model,
      system_prompt: form.systemPrompt,
      context_turns: form.contextTurns,
      max_output_tokens: form.maxOutputTokens,
      endpoint: {
        name: form.name,
        enabled: form.endpointEnabled,
        config: endpointConfig(),
        secrets: endpointSecrets()
      }
    }
  }

  function fillFromApp(app: AppRecord) {
    resetForm(app.app_type)
    form.step = 2
    createdApp.value = null
    form.name = app.name
    form.description = app.description
    form.status = app.status
    form.model = app.model
    lastAutoModel.value = app.model
    form.systemPrompt = app.system_prompt
    lastAutoSystemPrompt.value = app.system_prompt
    form.contextTurns = app.context_turns
    form.maxOutputTokens = app.max_output_tokens
    form.endpointEnabled = app.endpoint?.enabled ?? app.status === 'enabled'

    const config = app.endpoint?.config ?? {}
    if (app.app_type === 'wecom') {
      form.corpId = typeof config.corp_id === 'string' ? config.corp_id : ''
      form.agentId = typeof config.agent_id === 'string' ? config.agent_id : ''
    }
    if (app.app_type === 'feishu') {
      form.feishuAppId = typeof config.app_id === 'string' ? config.app_id : ''
    }
    if (app.app_type === 'widget') {
      form.allowedDomains = Array.isArray(config.allowed_domains)
        ? config.allowed_domains.map(String).join('\n')
        : ''
      form.welcome = typeof config.welcome === 'string' ? config.welcome : ''
      form.themeColor = typeof config.theme_color === 'string' ? config.theme_color : '#176baf'
      form.anonymousAccess = config.anonymous_access !== false
    }
  }

  async function loadModelOptions(userKeyId?: number) {
    return modelOptionsTask.run(
      () => getAppModelOptions({ userKeyId }),
      (options) => {
        modelOptions.value = options
        if (!form.model) applyRecommendedModel(true)
      }
    )
  }

  async function submitCreate(afterCreate?: () => Promise<void> | void) {
    await withLoading(saving, async () => {
      try {
        const app = await createApp(payload())
        createdApp.value = app
        form.step = 3
        ElMessage.success(t('appCreated'))
        await afterCreate?.()
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
  }

  return {
    appTypes,
    usageScenarios,
    form,
    modelOptions,
    saving,
    createdApp,
    createDialogTitle,
    selectedUsageScenario,
    canApplyScenarioPrompt,
    createdAccessUrls,
    applyUsageScenario,
    applySelectedScenarioPrompt,
    loadModelOptions,
    resetForm,
    fillFromApp,
    selectType,
    submitCreate,
    updatePayload,
    typeMeta,
    typeLabel
  }
}

export type UseAppCreate = ReturnType<typeof useAppCreate>
