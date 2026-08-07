/**
 * Single source of truth for the public interfaces docs sidebar menu and the
 * subsection route patterns (consumed by router/index.ts).
 */
export interface InterfacesMenuEntry {
  id: string
  path: string // Full route path, e.g. '/interfaces/openai/quick-start'
  label: { zh: string; en: string }
  children?: InterfacesMenuEntry[]
}

export const openAiSubSections = [
  'quick-start',
  'text',
  'text-async',
  'images',
  'images-async',
  'videos',
  'audio',
  'embeddings',
  'models',
  'sdk'
] as const

export const anthropicSubSections = ['quick-start', 'text', 'stream', 'batches', 'models'] as const

function openAiChild(id: string, label: { zh: string; en: string }): InterfacesMenuEntry {
  return { id: `openai-${id}`, path: `/interfaces/openai/${id}`, label }
}

function anthropicChild(id: string, label: { zh: string; en: string }): InterfacesMenuEntry {
  return { id: `anthropic-${id}`, path: `/interfaces/anthropic/${id}`, label }
}

export const interfacesMenu: InterfacesMenuEntry[] = [
  {
    id: 'before-start',
    path: '/interfaces/before-start',
    label: { zh: '接入前说明', en: 'Before You Start' }
  },
  {
    id: 'openai',
    path: '/interfaces/openai',
    label: { zh: 'OpenAI 兼容接口', en: 'OpenAI Compatible' },
    children: [
      openAiChild('quick-start', { zh: '快速开始', en: 'Quick start' }),
      openAiChild('text', { zh: '文本生成', en: 'Text generation' }),
      openAiChild('text-async', { zh: '文本生成（异步）', en: 'Text generation async' }),
      openAiChild('images', { zh: '图片生成', en: 'Images' }),
      openAiChild('images-async', { zh: '图片生成（异步）', en: 'Images async' }),
      openAiChild('videos', { zh: '视频与素材', en: 'Videos and assets' }),
      openAiChild('audio', { zh: '音频转写', en: 'Audio transcription' }),
      openAiChild('embeddings', { zh: '向量嵌入', en: 'Embeddings' }),
      openAiChild('models', { zh: '模型列表', en: 'Models' }),
      openAiChild('sdk', { zh: 'SDK 示例', en: 'SDK examples' })
    ]
  },
  {
    id: 'anthropic',
    path: '/interfaces/anthropic',
    label: { zh: 'Anthropic 兼容接口', en: 'Anthropic Compatible' },
    children: [
      anthropicChild('quick-start', { zh: '快速开始', en: 'Quick start' }),
      anthropicChild('text', { zh: '文本生成', en: 'Text generation' }),
      anthropicChild('stream', { zh: '流式输出', en: 'Streaming' }),
      anthropicChild('batches', { zh: '批量任务', en: 'Batch tasks' }),
      anthropicChild('models', { zh: '模型列表', en: 'Models' })
    ]
  },
  {
    id: 'errors',
    path: '/interfaces/errors',
    label: { zh: '错误码', en: 'Errors' }
  },
  {
    id: 'billing',
    path: '/interfaces/billing',
    label: { zh: '计费与用量', en: 'Billing and Usage' }
  }
]
