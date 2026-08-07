import { computed } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import { useSiteBrand } from '../../../composables/useSiteBrand'

export interface EndpointSample {
  title: string
  code: string
}

/**
 * Content for the Anthropic-compatible API docs section.
 * curl samples rely on BASE_URL / API_KEY exported in the quick-start example.
 */
export function useAnthropicContent() {
  const { locale } = useLocale()
  const { siteName } = useSiteBrand()
  const siteOrigin = computed(() => window.location.origin)

  const anthropicBaseUrl = computed(() => `${siteOrigin.value}/anthropic`)
  const anthropicBatchBaseUrl = computed(() => `${siteOrigin.value}/v1`)

  function isSupportedStatus(status: string) {
    return status.startsWith('已支持') || status.startsWith('Supported')
  }

  function endpointDescription(name: string, method: string, path: string) {
    const isZh = locale.value === 'zh-CN'
    const key = `${method} ${path}`

    const zhDescriptions: Record<string, string> = {
      'POST /v1/messages': '创建 Anthropic Messages 文本生成。',
      'POST /v1/messages/count_tokens': '预估 Messages 请求的 token 数。',
      'POST /v1/messages/batches': '创建 Message Batch 批量任务。',
      'GET /v1/messages/batches': '列出 Message Batch 批量任务。',
      'GET /v1/messages/batches/{message_batch_id}': '查询单个批量任务状态。',
      'POST /v1/messages/batches/{message_batch_id}/cancel': '取消批量任务。',
      'DELETE /v1/messages/batches/{message_batch_id}': '删除批量任务。',
      'GET /v1/messages/batches/{message_batch_id}/results': '读取批量任务结果。',
      'GET /v1/models': '列出 Anthropic 官方模型。',
      'GET /v1/models/{model_id}': '查询单个 Anthropic 官方模型。',
      'POST /v1/files': '上传文件资源。',
      'GET /v1/files': '列出文件资源。',
      'GET /v1/files/{file_id}': '查询文件元数据。',
      'DELETE /v1/files/{file_id}': '删除文件资源。',
      'GET /v1/files/{file_id}/content': '下载文件内容。'
    }

    const enDescriptions: Record<string, string> = {
      'POST /v1/messages': 'Create Anthropic Messages text generation.',
      'POST /v1/messages/count_tokens': 'Estimate token count for a Messages request.',
      'POST /v1/messages/batches': 'Create a Message Batch task.',
      'GET /v1/messages/batches': 'List Message Batch tasks.',
      'GET /v1/messages/batches/{message_batch_id}': 'Retrieve a single batch task.',
      'POST /v1/messages/batches/{message_batch_id}/cancel': 'Cancel a batch task.',
      'DELETE /v1/messages/batches/{message_batch_id}': 'Delete a batch task.',
      'GET /v1/messages/batches/{message_batch_id}/results': 'Read batch task results.',
      'GET /v1/models': 'List official Anthropic models.',
      'GET /v1/models/{model_id}': 'Retrieve an official Anthropic model.',
      'POST /v1/files': 'Upload a file resource.',
      'GET /v1/files': 'List file resources.',
      'GET /v1/files/{file_id}': 'Retrieve file metadata.',
      'DELETE /v1/files/{file_id}': 'Delete a file resource.',
      'GET /v1/files/{file_id}/content': 'Download file content.'
    }

    const fallback = isZh ? `${name} 接口功能。` : `${name} endpoint operation.`
    return (isZh ? zhDescriptions : enDescriptions)[key] ?? fallback
  }

  const quickStart = computed(() => {
    const apiKeyPlaceholder = locale.value === 'zh-CN' ? '<你的 API Key>' : '<your API key>'
    return `export BASE_URL="${siteOrigin.value}"
export API_KEY="${apiKeyPlaceholder}"

curl "$BASE_URL/anthropic/v1/messages" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 1024,
    "messages": [
      { "role": "user", "content": "用一句话介绍 NeoGate" }
    ]
  }'`
  })

  const messageSample = `curl "$BASE_URL/anthropic/v1/messages" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 1024,
    "system": "你是一个简洁的技术助手。",
    "messages": [
      { "role": "user", "content": "说明 NeoGate 的 Anthropic 兼容接口如何鉴权" }
    ]
  }'`

  const streamSample = `curl "$BASE_URL/anthropic/v1/messages" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 1024,
    "messages": [
      { "role": "user", "content": "连续输出 3 个要点" }
    ],
    "stream": true
  }'`

  const batchCreate = `curl "$BASE_URL/v1/messages/batches" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "requests": [
      {
        "custom_id": "summary-001",
        "params": {
          "model": "claude-3-5-sonnet-latest",
          "max_tokens": 1024,
          "messages": [
            { "role": "user", "content": "总结这段文本" }
          ]
        }
      }
    ]
  }'`

  const batchList = `curl "$BASE_URL/v1/messages/batches?limit=20" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01"`

  const batchRetrieve = `curl "$BASE_URL/v1/messages/batches/msgbatch_123" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01"`

  const batchCancel = `curl "$BASE_URL/v1/messages/batches/msgbatch_123/cancel" \\
  -X POST \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01"`

  const batchDelete = `curl "$BASE_URL/v1/messages/batches/msgbatch_123" \\
  -X DELETE \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01"`

  const batchResults = `curl "$BASE_URL/v1/messages/batches/msgbatch_123/results" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01"`

  const modelsSample = `curl "$BASE_URL/anthropic/v1/models" \\
  -H "x-api-key: $API_KEY" \\
  -H "anthropic-version: 2023-06-01"`

  const messageSamples: EndpointSample[] = [{ title: 'Messages', code: messageSample }]
  const streamSamples: EndpointSample[] = [{ title: 'Messages', code: streamSample }]
  const batchCreateSamples: EndpointSample[] = [{ title: 'Create Batch', code: batchCreate }]
  const batchManageSamples: EndpointSample[] = [
    { title: 'List Batches', code: batchList },
    { title: 'Retrieve Batch', code: batchRetrieve },
    { title: 'Cancel Batch', code: batchCancel },
    { title: 'Delete Batch', code: batchDelete }
  ]
  const batchResultsSamples: EndpointSample[] = [{ title: 'Batch Results', code: batchResults }]
  const modelsSamples: EndpointSample[] = [{ title: 'Models', code: modelsSample }]

  const content = computed(() => {
    if (locale.value === 'zh-CN') {
      return {
        anthropicTitle: '3. Anthropic 兼容接口',
        anthropicQuickStartTitle: '3.1 快速开始',
        anthropicTextTitle: '3.2 文本生成',
        anthropicStreamTitle: '3.3 流式输出',
        anthropicBatchesTitle: '3.4 批量任务',
        anthropicModelsTitle: '3.5 模型列表',
        urlPathsTitle: 'URL 路径',
        anthropicMessagePaths: [['POST', `${anthropicBaseUrl.value}/v1/messages`, 'Messages']],
        anthropicBatchPaths: [
          ['POST', `${anthropicBatchBaseUrl.value}/messages/batches`, '创建批量任务'],
          ['GET', `${anthropicBatchBaseUrl.value}/messages/batches`, '批量任务列表'],
          [
            'GET',
            `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}`,
            '查询批量任务'
          ],
          [
            'POST',
            `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}/cancel`,
            '取消批量任务'
          ],
          [
            'DELETE',
            `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}`,
            '删除批量任务'
          ],
          [
            'GET',
            `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}/results`,
            '读取批量任务结果'
          ]
        ],
        anthropicModelsPaths: [
          ['GET', `${anthropicBaseUrl.value}/v1/models`, '模型列表'],
          ['GET', `${anthropicBaseUrl.value}/v1/models/{model_id}`, '模型详情'],
          ['GET', `${anthropicBaseUrl.value}/v1/messages/models`, '兼容旧路径（模型列表）']
        ],
        anthropicMessageInterfaces: [
          {
            title: 'Messages',
            method: 'POST',
            path: `${anthropicBaseUrl.value}/v1/messages`,
            description: '创建 Anthropic Messages 文本生成。',
            requestParams: [
              ['model', 'string，必填', '请求模型名。'],
              ['max_tokens', 'integer，必填', '最大输出 token 数。'],
              ['messages', 'array，必填', 'Anthropic Messages 格式的对话消息。'],
              ['system', 'string | array', '系统提示。'],
              ['tools / tool_choice', 'array | object', '工具定义和工具选择策略。']
            ],
            responseFields: [
              ['id', 'string', '消息 ID。'],
              ['content[]', 'array', '输出内容块。'],
              ['stop_reason', 'string | null', '停止原因。'],
              ['usage', 'object', 'Token 用量。']
            ],
            samples: messageSamples
          }
        ],
        anthropicStreamInterfaces: [
          {
            title: 'Messages Stream',
            method: 'POST',
            path: `${anthropicBaseUrl.value}/v1/messages`,
            description: 'stream=true 时返回 Anthropic SSE 事件。',
            requestParams: [
              ['model', 'string，必填', '请求模型名。'],
              ['max_tokens', 'integer，必填', '最大输出 token 数。'],
              ['messages', 'array，必填', '对话消息。'],
              ['stream', 'boolean，必填 true', '启用流式输出。']
            ],
            responseFields: [
              ['message_start', 'event', '消息开始事件。'],
              ['content_block_delta', 'event', '内容增量事件。'],
              ['message_delta', 'event', '消息状态和用量增量。'],
              ['message_stop', 'event', '消息结束事件。']
            ],
            samples: streamSamples
          }
        ],
        anthropicBatchInterfaces: [
          {
            title: 'Create Batch',
            method: 'POST',
            path: `${anthropicBatchBaseUrl.value}/messages/batches`,
            description: '创建 Message Batch。',
            requestParams: [
              ['requests', 'array，必填', '批量任务条目数组。'],
              ['requests[].custom_id', 'string，必填', '调用方自定义 ID。'],
              ['requests[].params', 'object，必填', '单条 Messages 请求体。']
            ],
            responseFields: [
              ['id', 'string', '批量任务 ID。'],
              ['processing_status', 'string', '任务状态。'],
              ['request_counts', 'object', '各状态请求数量。']
            ],
            samples: batchCreateSamples
          },
          {
            title: 'List / Retrieve / Cancel / Delete Batch',
            method: 'GET / POST / DELETE',
            path: `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}`,
            description: '列出、查询、取消或删除批量任务。',
            requestParams: [
              ['message_batch_id', 'string', '批量任务 ID，查询单个任务时必填。'],
              ['limit / before_id / after_id', 'string | number', '列表分页参数。']
            ],
            responseFields: [
              ['data[]', 'array', '列表接口返回的批量任务数组。'],
              ['processing_status', 'string', '单个任务状态。'],
              ['request_counts', 'object', '各状态请求数量。']
            ],
            samples: batchManageSamples
          },
          {
            title: 'Batch Results',
            method: 'GET',
            path: `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}/results`,
            description: '读取批量任务结果流。',
            requestParams: [['message_batch_id', 'string，必填', '批量任务 ID。']],
            responseFields: [
              ['custom_id', 'string', '对应创建请求中的 custom_id。'],
              ['result.type', 'string', 'succeeded、errored、canceled 或 expired。'],
              ['result.message', 'object', '成功时的 Messages 响应。']
            ],
            samples: batchResultsSamples
          }
        ],
        anthropicModelsInterfaces: [
          {
            title: 'Models',
            method: 'GET',
            path: `${anthropicBaseUrl.value}/v1/models`,
            description: '读取当前 API Key 可调用的 Anthropic 模型列表。',
            requestParams: [
              ['limit', 'integer', '分页大小。'],
              ['before_id / after_id', 'string', '分页游标。']
            ],
            responseFields: [
              ['data[]', 'array', '模型列表。'],
              ['data[].id', 'string', '模型 ID。'],
              ['has_more', 'boolean', '是否还有下一页。']
            ],
            samples: modelsSamples
          }
        ],
        batchItems: [
          [
            '创建批量任务',
            '提交 requests 数组，每条请求使用 custom_id 标识，params 与 Messages 请求保持一致。'
          ],
          ['查询任务状态', '通过批量任务 ID 查询 processing_status、结果计数和过期时间。'],
          ['获取结果', '任务结束后下载 results，每行对应一条 custom_id 的处理结果。']
        ],
        anthropicModelsText:
          'Anthropic 官方 Models API 路径为 /v1/models 与 /v1/models/{model_id}，NeoGate 已在 /anthropic 前缀下提供，官方 SDK 的 models.list() / models.retrieve() 可直接使用。旧路径 /anthropic/v1/messages/models 仍保留以兼容既有调用方。',
        anthropicModelItems: [
          ['模型标识', '返回的 id 可直接作为 /v1/messages 的 model 参数。'],
          ['权限过滤', '列表会按当前 API Key 权限和后台启用渠道过滤。']
        ],
        paramFieldHeaders: ['参数', '类型', '说明'],
        endpointHeaders: ['模块', '方法', '官方路径', '接口说明', '状态'],
        anthropicIntro: `Anthropic 兼容接口使用 x-api-key 认证。下表仅列 Anthropic 官方 API 路径；${siteName.value} 的接入 Base URL、兼容扩展路径和示例在各小节中说明。`,
        anthropicAuthItems: [
          ['Messages Base URL', anthropicBaseUrl.value],
          ['Message Batches Base URL', anthropicBatchBaseUrl.value],
          ['认证头', 'x-api-key: YOUR_NEOGATE_API_KEY'],
          ['版本头', 'anthropic-version: 2023-06-01'],
          ['Beta 头', 'anthropic-beta 可按需透传']
        ],
        anthropicEndpoints: [
          [
            'Messages',
            'POST',
            '/v1/messages',
            'model, max_tokens, messages, system, tools, stream',
            '已支持',
            'text'
          ],
          [
            'Messages',
            'POST',
            '/v1/messages/count_tokens',
            'model, messages, system, tools',
            '已支持'
          ],
          [
            'Message Batches',
            'POST',
            '/v1/messages/batches',
            'requests[].custom_id, requests[].params',
            '已支持',
            'batches'
          ],
          [
            'Message Batches',
            'GET',
            '/v1/messages/batches',
            'limit, before_id, after_id',
            '已支持',
            'batches'
          ],
          [
            'Message Batches',
            'GET',
            '/v1/messages/batches/{message_batch_id}',
            'message_batch_id',
            '已支持',
            'batches'
          ],
          [
            'Message Batches',
            'POST',
            '/v1/messages/batches/{message_batch_id}/cancel',
            'message_batch_id',
            '已支持',
            'batches'
          ],
          [
            'Message Batches',
            'DELETE',
            '/v1/messages/batches/{message_batch_id}',
            'message_batch_id',
            '已支持',
            'batches'
          ],
          [
            'Message Batches',
            'GET',
            '/v1/messages/batches/{message_batch_id}/results',
            'message_batch_id',
            '已支持',
            'batches'
          ],
          ['Models', 'GET', '/v1/models', 'limit, before_id, after_id', '已支持', 'models'],
          ['Models', 'GET', '/v1/models/{model_id}', 'model_id', '已支持', 'models'],
          ['Files', 'POST', '/v1/files', 'file, purpose, anthropic-beta', '暂未支持'],
          ['Files', 'GET', '/v1/files', 'limit, before_id, after_id', '暂未支持'],
          ['Files', 'GET', '/v1/files/{file_id}', 'file_id', '暂未支持'],
          ['Files', 'DELETE', '/v1/files/{file_id}', 'file_id', '暂未支持'],
          ['Files', 'GET', '/v1/files/{file_id}/content', 'file_id', '暂未支持']
        ],
        anthropicText:
          'Messages 按 Anthropic 官方请求体转发。网关会按 model 路由到 Anthropic 或可桥接的 OpenAI 协议上游；system、tools、tool_choice、thinking、metadata、stop_sequences、temperature、top_p、top_k、stream 等字段会按兼容规则透传或转换。',
        messageRequestParams: [
          ['model', 'string', '必填。请求模型名，会匹配用户可调用的项目模型或已启用上游模型。'],
          ['max_tokens', 'integer', '必填。本次生成允许的最大输出 token 数。'],
          ['messages', 'array', '必填。Anthropic Messages 格式的对话消息。'],
          ['system', 'string | array', '可选。系统提示，支持文本块。'],
          ['tools', 'array', '可选。工具定义；兼容上游会转换为对应的工具调用格式。'],
          ['tool_choice', 'object | string', '可选。指定工具选择策略。'],
          [
            'thinking',
            'object',
            '可选。扩展思考配置；路由到 OpenAI 兼容上游时会映射为 reasoning。'
          ],
          ['stream', 'boolean', '可选。true 时返回 Anthropic SSE 事件。']
        ],
        messageResponseParams: [
          ['id', 'string', '消息 ID。'],
          ['type', 'string', '固定为 message。'],
          ['role', 'string', '固定为 assistant。'],
          ['content', 'array', '输出内容块，可能包含 text、tool_use 或 thinking。'],
          ['stop_reason', 'string | null', '停止原因，例如 end_turn、max_tokens、tool_use。'],
          ['usage', 'object', '输入、输出及缓存相关 token 用量。']
        ],
        streamText:
          '将 stream 设置为 true，响应会以 text/event-stream 返回 Anthropic 事件。网关会保留 message_start、content_block_delta、message_delta、message_stop 等事件形态，并记录最终用量。',
        batchText:
          'Message Batches 按官方接口创建、列出、查询、取消、删除和获取结果。创建请求中的每个 request 必须包含 custom_id 和 params，params 使用 Messages 请求体。批量任务需要可持久追踪的 Anthropic 上游凭证；创建后网关会跟踪终态并在结果可读时结算用量。',
        batchRequestParams: [
          ['requests', 'array', '必填。批量任务条目数组。'],
          ['requests[].custom_id', 'string', '必填。调用方自定义 ID，用于在 results 中对应结果。'],
          [
            'requests[].params',
            'object',
            '必填。单条 Messages 请求体，包含 model、max_tokens、messages 等字段。'
          ]
        ],
        batchResponseParams: [
          ['id', 'string', '批量任务 ID，用于查询、取消、删除和获取结果。'],
          ['processing_status', 'string', '任务状态，例如 in_progress、ended、canceling。'],
          ['request_counts', 'object', '处理中、成功、失败、取消、过期请求数量。'],
          [
            'results_url',
            'string | null',
            '上游返回的结果地址；NeoGate 通过 results 接口读取结果。'
          ]
        ],
        modelsResponseParams: [
          ['data', 'array', '模型列表。每项包含 id、type、display_name、created_at。'],
          ['first_id / last_id', 'string | null', '分页游标。'],
          ['has_more', 'boolean', '是否还有下一页。']
        ]
      }
    }

    return {
      anthropicTitle: '3. Anthropic-compatible APIs',
      anthropicQuickStartTitle: '3.1 Quick start',
      anthropicTextTitle: '3.2 Text generation',
      anthropicStreamTitle: '3.3 Streaming',
      anthropicBatchesTitle: '3.4 Batch tasks',
      anthropicModelsTitle: '3.5 Models',
      urlPathsTitle: 'URL paths',
      anthropicMessagePaths: [['POST', `${anthropicBaseUrl.value}/v1/messages`, 'Messages']],
      anthropicBatchPaths: [
        ['POST', `${anthropicBatchBaseUrl.value}/messages/batches`, 'Create batch'],
        ['GET', `${anthropicBatchBaseUrl.value}/messages/batches`, 'List batches'],
        [
          'GET',
          `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}`,
          'Retrieve batch'
        ],
        [
          'POST',
          `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}/cancel`,
          'Cancel batch'
        ],
        [
          'DELETE',
          `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}`,
          'Delete batch'
        ],
        [
          'GET',
          `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}/results`,
          'Read batch results'
        ]
      ],
      anthropicModelsPaths: [
        ['GET', `${anthropicBaseUrl.value}/v1/models`, 'List models'],
        ['GET', `${anthropicBaseUrl.value}/v1/models/{model_id}`, 'Retrieve model'],
        ['GET', `${anthropicBaseUrl.value}/v1/messages/models`, 'Legacy path (list models)']
      ],
      anthropicMessageInterfaces: [
        {
          title: 'Messages',
          method: 'POST',
          path: `${anthropicBaseUrl.value}/v1/messages`,
          description: 'Create Anthropic Messages text generation.',
          requestParams: [
            ['model', 'string, required', 'Requested model name.'],
            ['max_tokens', 'integer, required', 'Maximum output tokens.'],
            ['messages', 'array, required', 'Conversation messages in Anthropic Messages format.'],
            ['system', 'string | array', 'System prompt.'],
            [
              'tools / tool_choice',
              'array | object',
              'Tool definitions and tool selection strategy.'
            ]
          ],
          responseFields: [
            ['id', 'string', 'Message ID.'],
            ['content[]', 'array', 'Output content blocks.'],
            ['stop_reason', 'string | null', 'Stop reason.'],
            ['usage', 'object', 'Token usage.']
          ],
          samples: messageSamples
        }
      ],
      anthropicStreamInterfaces: [
        {
          title: 'Messages Stream',
          method: 'POST',
          path: `${anthropicBaseUrl.value}/v1/messages`,
          description: 'When stream=true, returns Anthropic SSE events.',
          requestParams: [
            ['model', 'string, required', 'Requested model name.'],
            ['max_tokens', 'integer, required', 'Maximum output tokens.'],
            ['messages', 'array, required', 'Conversation messages.'],
            ['stream', 'boolean, required true', 'Enables streaming output.']
          ],
          responseFields: [
            ['message_start', 'event', 'Message start event.'],
            ['content_block_delta', 'event', 'Content delta event.'],
            ['message_delta', 'event', 'Message status and usage delta.'],
            ['message_stop', 'event', 'Message stop event.']
          ],
          samples: streamSamples
        }
      ],
      anthropicBatchInterfaces: [
        {
          title: 'Create Batch',
          method: 'POST',
          path: `${anthropicBatchBaseUrl.value}/messages/batches`,
          description: 'Create a Message Batch.',
          requestParams: [
            ['requests', 'array, required', 'Batch request entries.'],
            ['requests[].custom_id', 'string, required', 'Caller-defined ID.'],
            ['requests[].params', 'object, required', 'Single Messages request body.']
          ],
          responseFields: [
            ['id', 'string', 'Batch ID.'],
            ['processing_status', 'string', 'Task status.'],
            ['request_counts', 'object', 'Request counts by status.']
          ],
          samples: batchCreateSamples
        },
        {
          title: 'List / Retrieve / Cancel / Delete Batch',
          method: 'GET / POST / DELETE',
          path: `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}`,
          description: 'List, retrieve, cancel, or delete batch tasks.',
          requestParams: [
            ['message_batch_id', 'string', 'Batch ID, required for single-task operations.'],
            ['limit / before_id / after_id', 'string | number', 'List pagination parameters.']
          ],
          responseFields: [
            ['data[]', 'array', 'Batch list returned by the list endpoint.'],
            ['processing_status', 'string', 'Single task status.'],
            ['request_counts', 'object', 'Request counts by status.']
          ],
          samples: batchManageSamples
        },
        {
          title: 'Batch Results',
          method: 'GET',
          path: `${anthropicBatchBaseUrl.value}/messages/batches/{message_batch_id}/results`,
          description: 'Read batch result stream.',
          requestParams: [['message_batch_id', 'string, required', 'Batch ID.']],
          responseFields: [
            ['custom_id', 'string', 'Matches the custom_id from the create request.'],
            ['result.type', 'string', 'succeeded, errored, canceled, or expired.'],
            ['result.message', 'object', 'Messages response when succeeded.']
          ],
          samples: batchResultsSamples
        }
      ],
      anthropicModelsInterfaces: [
        {
          title: 'Models',
          method: 'GET',
          path: `${anthropicBaseUrl.value}/v1/models`,
          description: 'List Anthropic models callable by the current API key.',
          requestParams: [
            ['limit', 'integer', 'Page size.'],
            ['before_id / after_id', 'string', 'Pagination cursor.']
          ],
          responseFields: [
            ['data[]', 'array', 'Model list.'],
            ['data[].id', 'string', 'Model ID.'],
            ['has_more', 'boolean', 'Whether another page is available.']
          ],
          samples: modelsSamples
        }
      ],
      batchItems: [
        [
          'Create a batch',
          'Submit a requests array. Each item uses custom_id, and params follows the Messages request shape.'
        ],
        ['Check status', 'Retrieve processing_status, result counts, and expiration by batch ID.'],
        [
          'Fetch results',
          'After completion, download results where each line maps to a custom_id result.'
        ]
      ],
      anthropicModelsText:
        'The official Anthropic Models API paths are /v1/models and /v1/models/{model_id}. NeoGate now serves both under the /anthropic prefix, so the official SDK models.list() / models.retrieve() work directly. The legacy /anthropic/v1/messages/models path is kept for backward compatibility.',
      anthropicModelItems: [
        ['Model IDs', 'Returned ids can be used directly as the model parameter for /v1/messages.'],
        [
          'Permission filtering',
          'The list is filtered by the current API key permissions and enabled upstream channels.'
        ]
      ],
      paramFieldHeaders: ['Parameter', 'Type', 'Description'],
      endpointHeaders: ['Module', 'Method', 'Official path', 'Description', 'Status'],
      anthropicIntro: `Anthropic-compatible APIs use x-api-key auth. The table lists only official Anthropic API paths; ${siteName.value} Base URLs, compatibility extension paths, and runnable examples are described in each section.`,
      anthropicAuthItems: [
        ['Messages Base URL', anthropicBaseUrl.value],
        ['Message Batches Base URL', anthropicBatchBaseUrl.value],
        ['Auth header', 'x-api-key: YOUR_NEOGATE_API_KEY'],
        ['Version header', 'anthropic-version: 2023-06-01'],
        ['Beta header', 'anthropic-beta is passed through when supplied']
      ],
      anthropicEndpoints: [
        [
          'Messages',
          'POST',
          '/v1/messages',
          'model, max_tokens, messages, system, tools, stream',
          'Supported',
          'text'
        ],
        [
          'Messages',
          'POST',
          '/v1/messages/count_tokens',
          'model, messages, system, tools',
          'Supported'
        ],
        [
          'Message Batches',
          'POST',
          '/v1/messages/batches',
          'requests[].custom_id, requests[].params',
          'Supported',
          'batches'
        ],
        [
          'Message Batches',
          'GET',
          '/v1/messages/batches',
          'limit, before_id, after_id',
          'Supported',
          'batches'
        ],
        [
          'Message Batches',
          'GET',
          '/v1/messages/batches/{message_batch_id}',
          'message_batch_id',
          'Supported',
          'batches'
        ],
        [
          'Message Batches',
          'POST',
          '/v1/messages/batches/{message_batch_id}/cancel',
          'message_batch_id',
          'Supported',
          'batches'
        ],
        [
          'Message Batches',
          'DELETE',
          '/v1/messages/batches/{message_batch_id}',
          'message_batch_id',
          'Supported',
          'batches'
        ],
        [
          'Message Batches',
          'GET',
          '/v1/messages/batches/{message_batch_id}/results',
          'message_batch_id',
          'Supported',
          'batches'
        ],
        ['Models', 'GET', '/v1/models', 'limit, before_id, after_id', 'Supported', 'models'],
        ['Models', 'GET', '/v1/models/{model_id}', 'model_id', 'Supported', 'models'],
        ['Files', 'POST', '/v1/files', 'file, purpose, anthropic-beta', 'Not supported'],
        ['Files', 'GET', '/v1/files', 'limit, before_id, after_id', 'Not supported'],
        ['Files', 'GET', '/v1/files/{file_id}', 'file_id', 'Not supported'],
        ['Files', 'DELETE', '/v1/files/{file_id}', 'file_id', 'Not supported'],
        ['Files', 'GET', '/v1/files/{file_id}/content', 'file_id', 'Not supported']
      ],
      anthropicText:
        'Messages are forwarded with the official Anthropic request body. The gateway routes by model to Anthropic or bridgeable OpenAI-protocol upstreams; system, tools, tool_choice, thinking, metadata, stop_sequences, temperature, top_p, top_k, and stream are passed through or converted by compatibility rules.',
      messageRequestParams: [
        [
          'model',
          'string',
          'Required. The requested model, matched against project or enabled upstream models.'
        ],
        ['max_tokens', 'integer', 'Required. Maximum output tokens for the generation.'],
        ['messages', 'array', 'Required. Conversation messages in Anthropic Messages format.'],
        ['system', 'string | array', 'Optional system prompt, including text blocks.'],
        [
          'tools',
          'array',
          'Optional tool definitions; bridgeable upstreams are converted to their tool format.'
        ],
        ['tool_choice', 'object | string', 'Optional tool selection strategy.'],
        [
          'thinking',
          'object',
          'Optional extended thinking configuration; mapped to reasoning for OpenAI-compatible upstreams.'
        ],
        ['stream', 'boolean', 'Optional. When true, returns Anthropic SSE events.']
      ],
      messageResponseParams: [
        ['id', 'string', 'Message ID.'],
        ['type', 'string', 'Always message.'],
        ['role', 'string', 'Always assistant.'],
        ['content', 'array', 'Output content blocks such as text, tool_use, or thinking.'],
        ['stop_reason', 'string | null', 'Stop reason such as end_turn, max_tokens, or tool_use.'],
        ['usage', 'object', 'Input, output, and cache-related token usage.']
      ],
      streamText:
        'Set stream to true to receive a text/event-stream response. The gateway preserves Anthropic event shapes such as message_start, content_block_delta, message_delta, and message_stop, and records final usage.',
      batchText:
        'Message Batches support create, list, retrieve, cancel, delete, and results. Each create request entry must include custom_id and params; params uses the Messages request body. Batch tasks require a key-backed Anthropic upstream that can be tracked persistently; NeoGate follows terminal state and settles usage when results are readable.',
      batchRequestParams: [
        ['requests', 'array', 'Required. Batch request entries.'],
        [
          'requests[].custom_id',
          'string',
          'Required caller-defined ID used to match result lines.'
        ],
        [
          'requests[].params',
          'object',
          'Required single Messages request body with model, max_tokens, messages, and related fields.'
        ]
      ],
      batchResponseParams: [
        ['id', 'string', 'Batch ID used for retrieve, cancel, delete, and results.'],
        ['processing_status', 'string', 'Task status such as in_progress, ended, or canceling.'],
        [
          'request_counts',
          'object',
          'Processing, succeeded, errored, canceled, and expired request counts.'
        ],
        [
          'results_url',
          'string | null',
          'Upstream results URL; NeoGate reads results through the results endpoint.'
        ]
      ],
      modelsResponseParams: [
        ['data', 'array', 'Model list. Each item includes id, type, display_name, and created_at.'],
        ['first_id / last_id', 'string | null', 'Pagination cursors.'],
        ['has_more', 'boolean', 'Whether another page is available.']
      ]
    }
  })

  return {
    content,
    quickStart,
    isSupportedStatus,
    endpointDescription
  }
}
