import { computed, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import {
  createApp,
  getAppModelOptions,
  type AppModelOption,
  type CreateAppInput,
  type UpdateAppInput
} from '../api/apps'
import type { AppRecord, AppType } from '../types/admin'
import { readError } from '../utils/errors'

export const appTypes = [
  {
    type: 'wecom',
    label: '企业微信应用',
    description: '接入企业微信自建应用，员工在企业微信里直接提问。',
    iconUrl: '/icons/wecom.svg',
    enabled: true
  },
  {
    type: 'feishu',
    label: '飞书应用',
    description: '接入飞书应用事件订阅，在飞书会话中自动回复消息。',
    iconUrl: '/icons/feishu.svg',
    enabled: true
  },
  {
    type: 'dingtalk',
    label: '钉钉应用',
    description: '接入钉钉机器人，用于群聊或单聊中的智能问答。',
    iconUrl: '/icons/dingtalk.svg',
    enabled: true
  },
  {
    type: 'webhook',
    label: 'Webhook 应用',
    description: '给外部系统或脚本调用，用 AI 处理事件、告警和工单。',
    iconUrl: '/icons/webhook.svg',
    enabled: true
  },
  {
    type: 'widget',
    label: '网页组件应用',
    description: '嵌入网站或内部页面，为访问者提供网页聊天入口。',
    iconUrl: '/icons/widget.svg',
    enabled: true
  }
] as const

export const usageScenarios = [
  {
    value: 'brief_qa',
    label: '简短问答',
    description: 'FAQ、轻量咨询',
    contextTurns: 4,
    maxOutputTokens: 1024,
    systemPrompt:
      '你是一个简洁准确的问答助手。优先直接回答用户问题，避免冗长解释。遇到不确定的信息时，说明不确定并给出可执行的下一步建议。'
  },
  {
    value: 'customer_support',
    label: '客服助手',
    description: '多轮追问、售前售后',
    contextTurns: 8,
    maxOutputTokens: 1536,
    systemPrompt:
      '你是一个专业、耐心的客服助手。先理解用户诉求，再给出清晰步骤。回答要友好、具体、可执行；遇到账号、订单、付款等敏感问题时，引导用户联系人工处理。'
  },
  {
    value: 'knowledge',
    label: '内部知识助手',
    description: '制度、文档、研发知识库',
    contextTurns: 10,
    maxOutputTokens: 2048,
    systemPrompt:
      '你是企业内部知识助手。根据已有知识和上下文回答问题，优先给出准确、结构清晰的结论。遇到缺少资料或不确定的问题时，明确说明不确定，不要编造。'
  },
  {
    value: 'analysis',
    label: '深度分析',
    description: '报告总结、复杂推理',
    contextTurns: 16,
    maxOutputTokens: 4096,
    systemPrompt:
      '你是一个严谨的分析助手。回答前先梳理问题目标和关键约束，再给出结构化分析、判断依据和建议。对不确定因素要明确标注，并给出可验证的后续步骤。'
  },
  {
    value: 'notification',
    label: '简短通知',
    description: 'Webhook 推送、状态解释',
    contextTurns: 2,
    maxOutputTokens: 512,
    systemPrompt:
      '你是一个简短通知助手。根据输入内容生成简洁、清楚、适合即时消息阅读的回复。优先保留关键信息、状态、时间和下一步动作，避免长篇解释。'
  }
] as const

export type UsageScenario = (typeof usageScenarios)[number]['value']

export function typeMeta(type: string) {
  return appTypes.find((item) => item.type === type) ?? appTypes[0]
}

export function typeLabel(type: string) {
  return typeMeta(type).label
}

export function statusLabel(status: string) {
  return status === 'enabled' ? '已启用' : '已禁用'
}

export function useAppCreate() {
  const modelOptions = ref<AppModelOption[]>([])
  const saving = ref(false)
  const createdApp = ref<AppRecord | null>(null)
  const lastAutoSystemPrompt = ref('')
  const lastAutoModel = ref('')

  const form = reactive({
    step: 1,
    appType: 'wecom' as AppType,
    name: '',
    description: '',
    status: 'enabled',
    model: '',
    usageScenario: 'knowledge' as UsageScenario,
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
      return `${typeLabel(createdApp.value?.app_type ?? form.appType)}接入信息`
    }
    return form.step === 2 ? `新建${typeLabel(form.appType)}` : '新建应用'
  })
  const selectedUsageScenario = computed(
    () => usageScenarios.find((item) => item.value === form.usageScenario) ?? usageScenarios[2]
  )
  const canApplyScenarioPrompt = computed(
    () => form.systemPrompt !== selectedUsageScenario.value.systemPrompt
  )
  const modelCandidates = computed(() => modelOptions.value.map((item) => item.model))
  const createdEndpoint = computed(() => createdApp.value?.endpoint ?? null)
  const createdAccessUrls = computed(() => {
    const endpoint = createdEndpoint.value
    if (!endpoint) return []
    if (endpoint.endpoint_type === 'wecom') {
      return [
        {
          label: '接收消息 URL',
          value: endpoint.callback_url,
          helper: '复制到企业微信应用的接收消息 URL。Token 和 EncodingAESKey 使用刚才填写的值。'
        }
      ]
    }
    if (endpoint.endpoint_type === 'feishu') {
      return [
        {
          label: '事件订阅请求地址',
          value: endpoint.callback_url,
          helper:
            '复制到飞书开发者后台 > 事件订阅 > 请求地址。Verification Token 和 Encrypt Key 使用刚才填写的值。'
        }
      ]
    }
    if (endpoint.endpoint_type === 'dingtalk') {
      return [
        {
          label: '机器人消息接收地址',
          value: endpoint.callback_url,
          helper: '复制到钉钉开发者后台的机器人消息接收地址，加签密钥使用刚才填写的值。'
        }
      ]
    }
    if (endpoint.endpoint_type === 'webhook') {
      return [
        {
          label: 'Webhook URL',
          value: endpoint.invoke_url,
          helper: '外部系统向这个地址发送请求。'
        }
      ]
    }
    if (endpoint.endpoint_type === 'widget') {
      return [
        {
          label: '嵌入脚本',
          value: endpoint.widget_script_url,
          helper: '把脚本地址加入允许域名下的页面。'
        },
        {
          label: '消息接口',
          value: endpoint.invoke_url,
          helper: '网页组件向这个地址发送消息。'
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
    form.contextTurns = scenario.contextTurns
    form.maxOutputTokens = scenario.maxOutputTokens
    if (
      options.forcePrompt ||
      !form.systemPrompt.trim() ||
      form.systemPrompt === lastAutoSystemPrompt.value
    ) {
      form.systemPrompt = scenario.systemPrompt
      lastAutoSystemPrompt.value = scenario.systemPrompt
    }
    applyRecommendedModel(options.forcePrompt)
  }

  function applySelectedScenarioPrompt() {
    form.systemPrompt = selectedUsageScenario.value.systemPrompt
    lastAutoSystemPrompt.value = selectedUsageScenario.value.systemPrompt
  }

  function recommendedModelForScenario() {
    const candidates = modelCandidates.value
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
    form.usageScenario = 'knowledge'
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
      ElMessage.info('该应用类型即将支持。')
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

  async function loadModelOptions() {
    modelOptions.value = await getAppModelOptions()
    if (!form.model) applyRecommendedModel(true)
  }

  async function submitCreate(afterCreate?: () => Promise<void> | void) {
    saving.value = true
    try {
      const app = await createApp(payload())
      createdApp.value = app
      form.step = 3
      ElMessage.success('应用已创建。')
      await afterCreate?.()
    } catch (err) {
      ElMessage.error(readError(err))
    } finally {
      saving.value = false
    }
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
