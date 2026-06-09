<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy, UserFilled } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import LocaleToggleButton from '../../components/LocaleToggleButton.vue'
import { useLocale } from '../../composables/useLocale'
import { useAuthStore } from '../../stores/auth'

const githubUrl = 'https://github.com/neogate-io/NeoGate'
const auth = useAuthStore()
const { locale, t } = useLocale()
const dashboardLink = computed(() => (auth.isAdmin ? '/admin' : '/home/overview'))
const siteOrigin = computed(() => window.location.origin)
const openAiBaseUrl = computed(() => `${siteOrigin.value}/v1`)
const anthropicBaseUrl = computed(() => `${siteOrigin.value}/anthropic`)

function scrollToSection(id: string) {
  document.getElementById(id)?.scrollIntoView({
    behavior: 'smooth',
    block: 'start'
  })
}

async function copyDocText(text: string) {
  await navigator.clipboard.writeText(text)
  ElMessage.success(t('apiKeyCopied'))
}

const openAiQuickStart = computed(
  () => `curl ${openAiBaseUrl.value}/chat/completions \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [
      { "role": "user", "content": "用一句话介绍 NeoGate" }
    ]
  }'`
)

const openAiStream = computed(
  () => `curl ${openAiBaseUrl.value}/chat/completions \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [
      { "role": "user", "content": "连续输出 3 个要点" }
    ],
    "stream": true
  }'`
)

const openAiResponses = computed(
  () => `curl ${openAiBaseUrl.value}/responses \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "写一个 TypeScript 防抖函数",
    "stream": false
  }'`
)

const openAiModels = computed(
  () => `curl ${openAiBaseUrl.value}/models \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const openAiPython = computed(
  () => `from openai import OpenAI

client = OpenAI(
    api_key="YOUR_NEOGATE_API_KEY",
    base_url="${openAiBaseUrl.value}",
)

response = client.chat.completions.create(
    model="gpt-5.5",
    messages=[
        {"role": "user", "content": "用一句话介绍 NeoGate"}
    ],
)

print(response.choices[0].message.content)`
)

const openAiNode = computed(
  () => `import OpenAI from "openai";

const client = new OpenAI({
  apiKey: "YOUR_NEOGATE_API_KEY",
  baseURL: "${openAiBaseUrl.value}",
});

const response = await client.responses.create({
  model: "gpt-5.5",
  input: "写一个 TypeScript 防抖函数",
});

console.log(response.output_text);`
)

const anthropicQuickStart = computed(
  () => `curl ${anthropicBaseUrl.value}/v1/messages \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 1024,
    "messages": [
      { "role": "user", "content": "用一句话介绍 NeoGate" }
    ]
  }'`
)

const anthropicStream = computed(
  () => `curl ${anthropicBaseUrl.value}/v1/messages \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY" \\
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
)

const anthropicBatch = computed(
  () => `curl ${siteOrigin.value}/v1/messages/batches \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY" \\
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
)

const anthropicModels = computed(
  () => `curl ${anthropicBaseUrl.value}/v1/messages/models \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY"`
)

const openAiResponseRetrieve = computed(
  () => `curl ${openAiBaseUrl.value}/responses/resp_123 \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const openAiResponseCancel = computed(
  () => `curl ${openAiBaseUrl.value}/responses/resp_123/cancel \\
  -X POST \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const anthropicBatchRetrieve = computed(
  () => `curl ${siteOrigin.value}/v1/messages/batches/msgbatch_123 \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY" \\
  -H "anthropic-version: 2023-06-01"`
)

const anthropicBatchResults = computed(
  () => `curl ${siteOrigin.value}/v1/messages/batches/msgbatch_123/results \\
  -H "x-api-key: YOUR_NEOGATE_API_KEY" \\
  -H "anthropic-version: 2023-06-01"`
)

const errorExample = `{
  "error": {
    "message": "insufficient credit",
    "type": "invalid_request_error"
  }
}`

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      title: '接口文档',
      subtitle:
        'NeoGate 按 OpenAI / Anthropic 官方 API 组织接口文档。已实现的接口可直接调用，尚未实现的官方接口统一标记为开发中。',
      menuTitle: '目录',
      menu: [
        ['before-start', '接入前说明', '1. 接入前说明'],
        ['openai', 'OpenAI 兼容接口', '2. OpenAI 兼容接口'],
        ['openai-quick-start', '快速开始', '2.1 快速开始', 'sub'],
        ['openai-text', '文本生成', '2.2 文本生成', 'sub'],
        ['openai-stream', '流式输出', '2.3 流式输出', 'sub'],
        ['openai-async', '异步任务', '2.4 异步任务', 'sub'],
        ['openai-images', '图片与多媒体', '2.5 图片与多媒体', 'sub'],
        ['openai-models', '模型与文件', '2.6 模型与文件', 'sub'],
        ['openai-sdk', 'SDK 示例', '2.7 SDK 示例', 'sub'],
        ['anthropic', 'Anthropic 兼容接口', '3. Anthropic 兼容接口'],
        ['anthropic-quick-start', '快速开始', '3.1 快速开始', 'sub'],
        ['anthropic-text', '文本生成', '3.2 文本生成', 'sub'],
        ['anthropic-stream', '流式输出', '3.3 流式输出', 'sub'],
        ['anthropic-batches', '批量任务', '3.4 批量任务', 'sub'],
        ['anthropic-models', '模型列表', '3.5 模型列表', 'sub'],
        ['errors', '错误码', '4. 错误码'],
        ['billing', '计费与用量', '5. 计费与用量']
      ],
      protocolTitle: '选择接入协议',
      protocolCards: [
        [
          'OpenAI 兼容',
          openAiBaseUrl.value,
          '适合 OpenAI SDK、Chat Completions、Responses、Codex 与多数 OpenAI 生态工具。'
        ],
        [
          'Anthropic 兼容',
          anthropicBaseUrl.value,
          '适合 Claude SDK、Claude Code、Messages 与 Message Batches。'
        ]
      ],
      beforeIntro:
        '下游应用只需要使用 NeoGate Base URL 和自己的 NeoGate API Key。上游供应商密钥、渠道健康状态和路由策略由后台统一管理。',
      beforeItems: [
        ['API Key', '用户可在用户后台创建 API Key，也可在开放注册时从公开首页领取。'],
        ['模型路由', '请求中的 model 会匹配已启用上游服务；多个渠道命中时由网关选择可用渠道。'],
        ['权限与额度', 'API Key 被停用、用户余额不足或模型不在允许范围内时，请求会被拒绝。'],
        ['用量记录', '网关会记录模型、Token、费用、首字延迟、总延迟和失败摘要。']
      ],
      endpointHeaders: ['模块', '方法', '官方路径', '关键参数', '状态'],
      openAiIntro:
        'OpenAI 兼容接口统一使用 Bearer Token 认证。Base URL 填写 NeoGate 的 /v1 地址。下表按 OpenAI 官方 API reference 列出接口族；未实现接口标记为开发中。',
      openAiAuthItems: [
        ['Base URL', openAiBaseUrl.value],
        ['认证头', 'Authorization: Bearer YOUR_NEOGATE_API_KEY'],
        ['Content-Type', 'application/json']
      ],
      openAiEndpoints: [
        ['Models', 'GET', '/v1/models', '-', '已支持'],
        ['Models', 'GET', '/v1/models/{model}', 'model', '开发中'],
        ['Models', 'DELETE', '/v1/models/{model}', 'model', '开发中'],
        ['Chat Completions', 'POST', '/v1/chat/completions', 'model, messages, stream', '已支持'],
        [
          'Chat Completions',
          'GET',
          '/v1/chat/completions/{completion_id}',
          'completion_id',
          '开发中'
        ],
        [
          'Chat Completions',
          'GET',
          '/v1/chat/completions/{completion_id}/messages',
          'completion_id',
          '开发中'
        ],
        [
          'Chat Completions',
          'PATCH',
          '/v1/chat/completions/{completion_id}',
          'completion_id, metadata',
          '开发中'
        ],
        [
          'Chat Completions',
          'DELETE',
          '/v1/chat/completions/{completion_id}',
          'completion_id',
          '开发中'
        ],
        ['Responses', 'POST', '/v1/responses', 'model, input, stream, background, store', '已支持'],
        [
          'Responses',
          'GET',
          '/v1/responses/{response_id}',
          'response_id, stream, starting_after',
          '已支持'
        ],
        ['Responses', 'DELETE', '/v1/responses/{response_id}', 'response_id', '开发中'],
        ['Responses', 'POST', '/v1/responses/{response_id}/cancel', 'response_id', '已支持'],
        [
          'Responses',
          'GET',
          '/v1/responses/{response_id}/input_items',
          'response_id, limit, after',
          '开发中'
        ],
        ['Images', 'POST', '/v1/images/generations', 'model, prompt, size, quality, n', '开发中'],
        ['Images', 'POST', '/v1/images/edits', 'model, image, prompt, mask, size, n', '开发中'],
        ['Images', 'POST', '/v1/images/variations', 'model, image, size, n', '开发中'],
        [
          'Embeddings',
          'POST',
          '/v1/embeddings',
          'model, input, dimensions, encoding_format',
          '开发中'
        ],
        ['Audio', 'POST', '/v1/audio/speech', 'model, input, voice, response_format', '开发中'],
        [
          'Audio',
          'POST',
          '/v1/audio/transcriptions',
          'model, file, language, response_format',
          '开发中'
        ],
        ['Audio', 'POST', '/v1/audio/translations', 'model, file, response_format', '开发中'],
        ['Moderations', 'POST', '/v1/moderations', 'model, input', '开发中'],
        ['Files', 'POST', '/v1/files', 'file, purpose', '开发中'],
        ['Files', 'GET', '/v1/files', 'purpose, limit, after', '开发中'],
        ['Files', 'GET', '/v1/files/{file_id}', 'file_id', '开发中'],
        ['Files', 'DELETE', '/v1/files/{file_id}', 'file_id', '开发中'],
        ['Files', 'GET', '/v1/files/{file_id}/content', 'file_id', '开发中'],
        ['Uploads', 'POST', '/v1/uploads', 'purpose, filename, bytes, mime_type', '开发中'],
        ['Uploads', 'POST', '/v1/uploads/{upload_id}/parts', 'upload_id, data', '开发中'],
        ['Uploads', 'POST', '/v1/uploads/{upload_id}/complete', 'upload_id, part_ids', '开发中'],
        ['Uploads', 'POST', '/v1/uploads/{upload_id}/cancel', 'upload_id', '开发中'],
        ['Batches', 'POST', '/v1/batches', 'input_file_id, endpoint, completion_window', '开发中'],
        ['Batches', 'GET', '/v1/batches', 'limit, after', '开发中'],
        ['Batches', 'GET', '/v1/batches/{batch_id}', 'batch_id', '开发中'],
        ['Batches', 'POST', '/v1/batches/{batch_id}/cancel', 'batch_id', '开发中'],
        [
          'Fine-tuning',
          'POST',
          '/v1/fine_tuning/jobs',
          'model, training_file, validation_file, hyperparameters',
          '开发中'
        ],
        ['Fine-tuning', 'GET', '/v1/fine_tuning/jobs', 'limit, after', '开发中'],
        [
          'Fine-tuning',
          'GET',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}',
          'fine_tuning_job_id',
          '开发中'
        ],
        [
          'Fine-tuning',
          'POST',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}/cancel',
          'fine_tuning_job_id',
          '开发中'
        ],
        [
          'Fine-tuning',
          'GET',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}/events',
          'fine_tuning_job_id, limit, after',
          '开发中'
        ],
        [
          'Fine-tuning',
          'GET',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints',
          'fine_tuning_job_id, limit, after',
          '开发中'
        ],
        ['Vector Stores', 'POST', '/v1/vector_stores', 'name, file_ids, expires_after', '开发中'],
        ['Vector Stores', 'GET', '/v1/vector_stores', 'limit, after, before', '开发中'],
        [
          'Vector Stores',
          'GET/PATCH/DELETE',
          '/v1/vector_stores/{vector_store_id}',
          'vector_store_id',
          '开发中'
        ],
        [
          'Vector Store Files',
          'POST/GET',
          '/v1/vector_stores/{vector_store_id}/files',
          'vector_store_id, file_id',
          '开发中'
        ],
        [
          'Vector Store Files',
          'GET/DELETE',
          '/v1/vector_stores/{vector_store_id}/files/{file_id}',
          'vector_store_id, file_id',
          '开发中'
        ],
        [
          'Vector Store File Batches',
          'POST/GET',
          '/v1/vector_stores/{vector_store_id}/file_batches',
          'vector_store_id, file_ids',
          '开发中'
        ],
        [
          'Vector Store File Batches',
          'GET/POST',
          '/v1/vector_stores/{vector_store_id}/file_batches/{batch_id}',
          'vector_store_id, batch_id',
          '开发中'
        ],
        [
          'Assistants',
          'POST/GET',
          '/v1/assistants',
          'model, instructions, tools, metadata',
          '开发中'
        ],
        [
          'Assistants',
          'GET/PATCH/DELETE',
          '/v1/assistants/{assistant_id}',
          'assistant_id',
          '开发中'
        ],
        ['Threads', 'POST', '/v1/threads', 'messages, metadata, tool_resources', '开发中'],
        ['Threads', 'GET/PATCH/DELETE', '/v1/threads/{thread_id}', 'thread_id', '开发中'],
        [
          'Thread Messages',
          'POST/GET',
          '/v1/threads/{thread_id}/messages',
          'thread_id, role, content',
          '开发中'
        ],
        [
          'Thread Messages',
          'GET/PATCH/DELETE',
          '/v1/threads/{thread_id}/messages/{message_id}',
          'thread_id, message_id',
          '开发中'
        ],
        [
          'Thread Runs',
          'POST/GET',
          '/v1/threads/{thread_id}/runs',
          'thread_id, assistant_id, model',
          '开发中'
        ],
        [
          'Thread Runs',
          'GET/PATCH',
          '/v1/threads/{thread_id}/runs/{run_id}',
          'thread_id, run_id',
          '开发中'
        ],
        [
          'Thread Runs',
          'POST',
          '/v1/threads/{thread_id}/runs/{run_id}/cancel',
          'thread_id, run_id',
          '开发中'
        ],
        [
          'Thread Runs',
          'POST',
          '/v1/threads/{thread_id}/runs/{run_id}/submit_tool_outputs',
          'thread_id, run_id, tool_outputs',
          '开发中'
        ],
        [
          'Realtime',
          'POST',
          '/v1/realtime/sessions',
          'model, voice, modalities, instructions',
          '开发中'
        ],
        [
          'Realtime',
          'POST',
          '/v1/realtime/transcription_sessions',
          'input_audio_format, input_audio_transcription',
          '开发中'
        ],
        ['Evals', 'POST/GET', '/v1/evals', 'name, data_source_config, testing_criteria', '开发中'],
        ['Evals', 'GET/PATCH/DELETE', '/v1/evals/{eval_id}', 'eval_id', '开发中'],
        [
          'Eval Runs',
          'POST/GET',
          '/v1/evals/{eval_id}/runs',
          'eval_id, data_source, model',
          '开发中'
        ],
        [
          'Eval Runs',
          'GET/DELETE',
          '/v1/evals/{eval_id}/runs/{run_id}',
          'eval_id, run_id',
          '开发中'
        ]
      ],
      quickStartTitle: '快速开始',
      textTitle: '文本生成',
      openAiText:
        'Chat Completions 与 Responses 均按 OpenAI 官方请求体转发。NeoGate 当前支持 create；Chat Completions 的 stored completion 查询、更新、删除仍在开发中。',
      streamTitle: '流式输出',
      streamText: '将 stream 设置为 true，响应会以 text/event-stream 形式返回。',
      asyncTitle: '异步任务',
      openAiAsync:
        'Responses 后台任务按官方 background 参数创建。NeoGate 要求 background=true 时 store 不能为 false；创建后台任务时不支持直接 stream=true，可在查询接口透传 stream=true 获取后续流。',
      imageTitle: '图片生成',
      openAiImage:
        'OpenAI 官方 Images、Audio、Embeddings、Moderations、Files、Uploads、Batches、Fine-tuning、Vector Stores、Assistants、Threads、Realtime、Evals 等接口已列入总览，当前均为开发中。',
      modelsTitle: '模型列表',
      sdkTitle: 'SDK 示例',
      anthropicIntro:
        'Anthropic 兼容接口使用 x-api-key 认证。Base URL 填写 NeoGate 的 /anthropic 地址；/v1/messages 与 /v1/messages/batches 系列按 Anthropic 官方路径兼容。模型列表当前通过 NeoGate 扩展路径提供。',
      anthropicAuthItems: [
        ['Base URL', anthropicBaseUrl.value],
        ['认证头', 'x-api-key: YOUR_NEOGATE_API_KEY'],
        ['版本头', 'anthropic-version: 2023-06-01']
      ],
      anthropicEndpoints: [
        [
          'Messages',
          'POST',
          '/v1/messages',
          'model, max_tokens, messages, system, tools, stream',
          '已支持'
        ],
        [
          'Messages',
          'POST',
          '/v1/messages/count_tokens',
          'model, messages, system, tools',
          '开发中'
        ],
        [
          'Message Batches',
          'POST',
          '/v1/messages/batches',
          'requests[].custom_id, requests[].params',
          '已支持'
        ],
        ['Message Batches', 'GET', '/v1/messages/batches', 'limit, before_id, after_id', '已支持'],
        [
          'Message Batches',
          'GET',
          '/v1/messages/batches/{message_batch_id}',
          'message_batch_id',
          '已支持'
        ],
        [
          'Message Batches',
          'POST',
          '/v1/messages/batches/{message_batch_id}/cancel',
          'message_batch_id',
          '已支持'
        ],
        [
          'Message Batches',
          'DELETE',
          '/v1/messages/batches/{message_batch_id}',
          'message_batch_id',
          '已支持'
        ],
        [
          'Message Batches',
          'GET',
          '/v1/messages/batches/{message_batch_id}/results',
          'message_batch_id',
          '已支持'
        ],
        ['Models', 'GET', '/v1/models', 'limit, before_id, after_id', '开发中'],
        ['Models', 'GET', '/v1/models/{model_id}', 'model_id', '开发中'],
        [
          'Models',
          'GET',
          '/anthropic/v1/messages/models',
          'NeoGate 扩展：列出当前 API Key 可调用的 Anthropic 协议模型。',
          '已支持'
        ],
        [
          'Messages',
          'POST',
          '/anthropic/v1/messages',
          'NeoGate 扩展：Anthropic Base URL 下的 Messages 路径。',
          '已支持'
        ],
        ['Files', 'POST/GET', '/v1/files', 'file, purpose, limit, after', '开发中'],
        ['Files', 'GET/DELETE', '/v1/files/{file_id}', 'file_id', '开发中']
      ],
      anthropicText:
        'Messages 按 Anthropic 官方请求体转发。必填参数为 model、max_tokens、messages；system、tools、tool_choice、metadata、stop_sequences、temperature、top_p、top_k、stream 等按官方字段透传。',
      batchTitle: '批量任务',
      batchText:
        'Message Batches 按官方接口创建、查询、取消、删除和获取结果。创建请求中的每个 request 必须包含 custom_id 和 params，params 使用 Messages 请求体。',
      errorIntro: '错误响应为 JSON。不同上游可能返回不同 message，网关会尽量保留可排查的信息。',
      errorHeaders: ['HTTP 状态', '常见原因', '建议处理'],
      errors: [
        ['401', 'API Key 缺失或无效', '检查 Authorization 或 x-api-key 请求头。'],
        ['403', 'API Key 停用、账号未启用或模型受限', '确认用户状态、Key 状态和模型权限。'],
        [
          '400',
          '请求体格式错误、缺少字段或异步参数不满足要求',
          '检查 JSON、model、messages、store 等字段。'
        ],
        ['402/400', '余额不足或额度校验未通过', '充值或联系管理员调整额度策略。'],
        [
          '404/503',
          '模型没有可用渠道或上游服务不可用',
          '确认后台渠道模型、Key 健康状态和服务商可用性。'
        ],
        ['429/5xx', '上游限流、超时或返回失败', '稍后重试，或在后台切换/补充上游渠道。']
      ],
      billingIntro: 'NeoGate 会在转发前预估额度，在响应完成后按实际 Token 和价格结算。',
      billingItems: [
        ['文本生成', '按输入、输出以及缓存读写 Token 记录用量；价格由后台渠道价格与售价策略决定。'],
        ['流式请求', '流结束后统一结算；如果中途失败，会记录已知用量和失败摘要。'],
        ['异步/批量任务', '任务创建后会被跟踪，终态返回后结算用量。'],
        ['失败请求', '未转发到上游的请求通常不消耗上游额度，但会保留失败记录用于排查。']
      ]
    }
  }

  return {
    title: 'API Reference',
    subtitle:
      'NeoGate follows the official OpenAI / Anthropic API structure. Implemented APIs are callable now; official APIs not implemented yet are marked as in development.',
    menuTitle: 'Contents',
    menu: [
      ['before-start', 'Before You Start', '1. Before You Start'],
      ['openai', 'OpenAI Compatible', '2. OpenAI-compatible APIs'],
      ['openai-quick-start', 'Quick start', '2.1 Quick start', 'sub'],
      ['openai-text', 'Text generation', '2.2 Text generation', 'sub'],
      ['openai-stream', 'Streaming', '2.3 Streaming', 'sub'],
      ['openai-async', 'Async tasks', '2.4 Async tasks', 'sub'],
      ['openai-images', 'Images and media', '2.5 Images and media', 'sub'],
      ['openai-models', 'Models and files', '2.6 Models and files', 'sub'],
      ['openai-sdk', 'SDK examples', '2.7 SDK examples', 'sub'],
      ['anthropic', 'Anthropic Compatible', '3. Anthropic-compatible APIs'],
      ['anthropic-quick-start', 'Quick start', '3.1 Quick start', 'sub'],
      ['anthropic-text', 'Text generation', '3.2 Text generation', 'sub'],
      ['anthropic-stream', 'Streaming', '3.3 Streaming', 'sub'],
      ['anthropic-batches', 'Batch tasks', '3.4 Batch tasks', 'sub'],
      ['anthropic-models', 'Models', '3.5 Models', 'sub'],
      ['errors', 'Errors', '4. Errors'],
      ['billing', 'Billing and Usage', '5. Billing and Usage']
    ],
    protocolTitle: 'Choose a protocol',
    protocolCards: [
      [
        'OpenAI compatible',
        openAiBaseUrl.value,
        'For OpenAI SDKs, Chat Completions, Responses, Codex, and most OpenAI ecosystem tools.'
      ],
      [
        'Anthropic compatible',
        anthropicBaseUrl.value,
        'For Claude SDKs, Claude Code, Messages, and Message Batches.'
      ]
    ],
    beforeIntro:
      'Client apps only need the NeoGate Base URL and their NeoGate API key. Upstream credentials, channel health, and routing are managed in the admin console.',
    beforeItems: [
      [
        'API key',
        'Users can create API keys in the user console or request one from the public home page when registration is open.'
      ],
      [
        'Model routing',
        'The requested model is matched against enabled upstream services; when multiple channels match, the gateway selects an available channel.'
      ],
      [
        'Permission and credit',
        'Disabled keys, insufficient balance, or disallowed models reject the request.'
      ],
      [
        'Usage records',
        'The gateway records model, tokens, cost, first-token latency, total latency, and failure summaries.'
      ]
    ],
    endpointHeaders: ['Module', 'Method', 'Official path', 'Key parameters', 'Status'],
    openAiIntro:
      'OpenAI-compatible APIs use Bearer Token auth. Set the Base URL to the NeoGate /v1 URL. The table follows the official OpenAI API reference; unimplemented official APIs are marked as in development.',
    openAiAuthItems: [
      ['Base URL', openAiBaseUrl.value],
      ['Auth header', 'Authorization: Bearer YOUR_NEOGATE_API_KEY'],
      ['Content-Type', 'application/json']
    ],
    openAiEndpoints: [
      ['Models', 'GET', '/v1/models', '-', 'Supported'],
      ['Models', 'GET', '/v1/models/{model}', 'model', 'In development'],
      ['Models', 'DELETE', '/v1/models/{model}', 'model', 'In development'],
      ['Chat Completions', 'POST', '/v1/chat/completions', 'model, messages, stream', 'Supported'],
      [
        'Chat Completions',
        'GET',
        '/v1/chat/completions/{completion_id}',
        'completion_id',
        'In development'
      ],
      [
        'Chat Completions',
        'GET',
        '/v1/chat/completions/{completion_id}/messages',
        'completion_id',
        'In development'
      ],
      [
        'Chat Completions',
        'PATCH',
        '/v1/chat/completions/{completion_id}',
        'completion_id, metadata',
        'In development'
      ],
      [
        'Chat Completions',
        'DELETE',
        '/v1/chat/completions/{completion_id}',
        'completion_id',
        'In development'
      ],
      [
        'Responses',
        'POST',
        '/v1/responses',
        'model, input, stream, background, store',
        'Supported'
      ],
      [
        'Responses',
        'GET',
        '/v1/responses/{response_id}',
        'response_id, stream, starting_after',
        'Supported'
      ],
      ['Responses', 'DELETE', '/v1/responses/{response_id}', 'response_id', 'In development'],
      ['Responses', 'POST', '/v1/responses/{response_id}/cancel', 'response_id', 'Supported'],
      [
        'Responses',
        'GET',
        '/v1/responses/{response_id}/input_items',
        'response_id, limit, after',
        'In development'
      ],
      [
        'Images',
        'POST',
        '/v1/images/generations',
        'model, prompt, size, quality, n',
        'In development'
      ],
      [
        'Images',
        'POST',
        '/v1/images/edits',
        'model, image, prompt, mask, size, n',
        'In development'
      ],
      ['Images', 'POST', '/v1/images/variations', 'model, image, size, n', 'In development'],
      [
        'Embeddings',
        'POST',
        '/v1/embeddings',
        'model, input, dimensions, encoding_format',
        'In development'
      ],
      [
        'Audio',
        'POST',
        '/v1/audio/speech',
        'model, input, voice, response_format',
        'In development'
      ],
      [
        'Audio',
        'POST',
        '/v1/audio/transcriptions',
        'model, file, language, response_format',
        'In development'
      ],
      ['Audio', 'POST', '/v1/audio/translations', 'model, file, response_format', 'In development'],
      ['Moderations', 'POST', '/v1/moderations', 'model, input', 'In development'],
      ['Files', 'POST', '/v1/files', 'file, purpose', 'In development'],
      ['Files', 'GET', '/v1/files', 'purpose, limit, after', 'In development'],
      ['Files', 'GET', '/v1/files/{file_id}', 'file_id', 'In development'],
      ['Files', 'DELETE', '/v1/files/{file_id}', 'file_id', 'In development'],
      ['Files', 'GET', '/v1/files/{file_id}/content', 'file_id', 'In development'],
      ['Uploads', 'POST', '/v1/uploads', 'purpose, filename, bytes, mime_type', 'In development'],
      ['Uploads', 'POST', '/v1/uploads/{upload_id}/parts', 'upload_id, data', 'In development'],
      [
        'Uploads',
        'POST',
        '/v1/uploads/{upload_id}/complete',
        'upload_id, part_ids',
        'In development'
      ],
      ['Uploads', 'POST', '/v1/uploads/{upload_id}/cancel', 'upload_id', 'In development'],
      [
        'Batches',
        'POST',
        '/v1/batches',
        'input_file_id, endpoint, completion_window',
        'In development'
      ],
      ['Batches', 'GET', '/v1/batches', 'limit, after', 'In development'],
      ['Batches', 'GET', '/v1/batches/{batch_id}', 'batch_id', 'In development'],
      ['Batches', 'POST', '/v1/batches/{batch_id}/cancel', 'batch_id', 'In development'],
      [
        'Fine-tuning',
        'POST',
        '/v1/fine_tuning/jobs',
        'model, training_file, validation_file, hyperparameters',
        'In development'
      ],
      ['Fine-tuning', 'GET', '/v1/fine_tuning/jobs', 'limit, after', 'In development'],
      [
        'Fine-tuning',
        'GET',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}',
        'fine_tuning_job_id',
        'In development'
      ],
      [
        'Fine-tuning',
        'POST',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}/cancel',
        'fine_tuning_job_id',
        'In development'
      ],
      [
        'Fine-tuning',
        'GET',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}/events',
        'fine_tuning_job_id, limit, after',
        'In development'
      ],
      [
        'Fine-tuning',
        'GET',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints',
        'fine_tuning_job_id, limit, after',
        'In development'
      ],
      [
        'Vector Stores',
        'POST',
        '/v1/vector_stores',
        'name, file_ids, expires_after',
        'In development'
      ],
      ['Vector Stores', 'GET', '/v1/vector_stores', 'limit, after, before', 'In development'],
      [
        'Vector Stores',
        'GET/PATCH/DELETE',
        '/v1/vector_stores/{vector_store_id}',
        'vector_store_id',
        'In development'
      ],
      [
        'Vector Store Files',
        'POST/GET',
        '/v1/vector_stores/{vector_store_id}/files',
        'vector_store_id, file_id',
        'In development'
      ],
      [
        'Vector Store Files',
        'GET/DELETE',
        '/v1/vector_stores/{vector_store_id}/files/{file_id}',
        'vector_store_id, file_id',
        'In development'
      ],
      [
        'Vector Store File Batches',
        'POST/GET',
        '/v1/vector_stores/{vector_store_id}/file_batches',
        'vector_store_id, file_ids',
        'In development'
      ],
      [
        'Vector Store File Batches',
        'GET/POST',
        '/v1/vector_stores/{vector_store_id}/file_batches/{batch_id}',
        'vector_store_id, batch_id',
        'In development'
      ],
      [
        'Assistants',
        'POST/GET',
        '/v1/assistants',
        'model, instructions, tools, metadata',
        'In development'
      ],
      [
        'Assistants',
        'GET/PATCH/DELETE',
        '/v1/assistants/{assistant_id}',
        'assistant_id',
        'In development'
      ],
      ['Threads', 'POST', '/v1/threads', 'messages, metadata, tool_resources', 'In development'],
      ['Threads', 'GET/PATCH/DELETE', '/v1/threads/{thread_id}', 'thread_id', 'In development'],
      [
        'Thread Messages',
        'POST/GET',
        '/v1/threads/{thread_id}/messages',
        'thread_id, role, content',
        'In development'
      ],
      [
        'Thread Messages',
        'GET/PATCH/DELETE',
        '/v1/threads/{thread_id}/messages/{message_id}',
        'thread_id, message_id',
        'In development'
      ],
      [
        'Thread Runs',
        'POST/GET',
        '/v1/threads/{thread_id}/runs',
        'thread_id, assistant_id, model',
        'In development'
      ],
      [
        'Thread Runs',
        'GET/PATCH',
        '/v1/threads/{thread_id}/runs/{run_id}',
        'thread_id, run_id',
        'In development'
      ],
      [
        'Thread Runs',
        'POST',
        '/v1/threads/{thread_id}/runs/{run_id}/cancel',
        'thread_id, run_id',
        'In development'
      ],
      [
        'Thread Runs',
        'POST',
        '/v1/threads/{thread_id}/runs/{run_id}/submit_tool_outputs',
        'thread_id, run_id, tool_outputs',
        'In development'
      ],
      [
        'Realtime',
        'POST',
        '/v1/realtime/sessions',
        'model, voice, modalities, instructions',
        'In development'
      ],
      [
        'Realtime',
        'POST',
        '/v1/realtime/transcription_sessions',
        'input_audio_format, input_audio_transcription',
        'In development'
      ],
      [
        'Evals',
        'POST/GET',
        '/v1/evals',
        'name, data_source_config, testing_criteria',
        'In development'
      ],
      ['Evals', 'GET/PATCH/DELETE', '/v1/evals/{eval_id}', 'eval_id', 'In development'],
      [
        'Eval Runs',
        'POST/GET',
        '/v1/evals/{eval_id}/runs',
        'eval_id, data_source, model',
        'In development'
      ],
      [
        'Eval Runs',
        'GET/DELETE',
        '/v1/evals/{eval_id}/runs/{run_id}',
        'eval_id, run_id',
        'In development'
      ]
    ],
    quickStartTitle: 'Quick start',
    textTitle: 'Text generation',
    openAiText:
      'Chat Completions and Responses are forwarded with the official OpenAI request body. NeoGate currently supports create; stored Chat Completion retrieve, update, message listing, and delete are in development.',
    streamTitle: 'Streaming',
    streamText: 'Set stream to true to receive a text/event-stream response.',
    asyncTitle: 'Async tasks',
    openAiAsync:
      'Background Responses follow the official background parameter. NeoGate requires store not to be false when background=true. Create-time streaming is not supported for background tasks; retrieve can pass through stream=true.',
    imageTitle: 'Image generation',
    openAiImage:
      'Official Images, Audio, Embeddings, Moderations, Files, Uploads, Batches, Fine-tuning, Vector Stores, Assistants, Threads, Realtime, and Evals APIs are listed above and are currently in development.',
    modelsTitle: 'Models',
    sdkTitle: 'SDK examples',
    anthropicIntro:
      'Anthropic-compatible APIs use x-api-key auth. Set the Base URL to the NeoGate /anthropic URL. /v1/messages and /v1/messages/batches follow official Anthropic paths. Model listing is currently exposed through a NeoGate extension path.',
    anthropicAuthItems: [
      ['Base URL', anthropicBaseUrl.value],
      ['Auth header', 'x-api-key: YOUR_NEOGATE_API_KEY'],
      ['Version header', 'anthropic-version: 2023-06-01']
    ],
    anthropicEndpoints: [
      [
        'Messages',
        'POST',
        '/v1/messages',
        'model, max_tokens, messages, system, tools, stream',
        'Supported'
      ],
      [
        'Messages',
        'POST',
        '/v1/messages/count_tokens',
        'model, messages, system, tools',
        'In development'
      ],
      [
        'Message Batches',
        'POST',
        '/v1/messages/batches',
        'requests[].custom_id, requests[].params',
        'Supported'
      ],
      ['Message Batches', 'GET', '/v1/messages/batches', 'limit, before_id, after_id', 'Supported'],
      [
        'Message Batches',
        'GET',
        '/v1/messages/batches/{message_batch_id}',
        'message_batch_id',
        'Supported'
      ],
      [
        'Message Batches',
        'POST',
        '/v1/messages/batches/{message_batch_id}/cancel',
        'message_batch_id',
        'Supported'
      ],
      [
        'Message Batches',
        'DELETE',
        '/v1/messages/batches/{message_batch_id}',
        'message_batch_id',
        'Supported'
      ],
      [
        'Message Batches',
        'GET',
        '/v1/messages/batches/{message_batch_id}/results',
        'message_batch_id',
        'Supported'
      ],
      ['Models', 'GET', '/v1/models', 'limit, before_id, after_id', 'In development'],
      ['Models', 'GET', '/v1/models/{model_id}', 'model_id', 'In development'],
      [
        'Models',
        'GET',
        '/anthropic/v1/messages/models',
        'NeoGate extension: lists Anthropic-protocol models available to the API key.',
        'Supported'
      ],
      [
        'Messages',
        'POST',
        '/anthropic/v1/messages',
        'NeoGate extension under the Anthropic Base URL.',
        'Supported'
      ],
      ['Files', 'POST/GET', '/v1/files', 'file, purpose, limit, after', 'In development'],
      ['Files', 'GET/DELETE', '/v1/files/{file_id}', 'file_id', 'In development']
    ],
    anthropicText:
      'Messages are forwarded with the official Anthropic request body. Required fields are model, max_tokens, and messages; system, tools, tool_choice, metadata, stop_sequences, temperature, top_p, top_k, and stream are passed through.',
    batchTitle: 'Batch tasks',
    batchText:
      'Message Batches support create, list, retrieve, cancel, delete, and results. Each create request entry must include custom_id and params; params uses the Messages request body.',
    errorIntro:
      'Error responses are JSON. Upstream providers may return different messages; the gateway keeps useful diagnostic information where possible.',
    errorHeaders: ['HTTP status', 'Common reason', 'Suggested handling'],
    errors: [
      ['401', 'Missing or invalid API key', 'Check the Authorization or x-api-key request header.'],
      [
        '403',
        'Disabled key, disabled account, or model restriction',
        'Check user status, key status, and model permissions.'
      ],
      [
        '400',
        'Invalid request body, missing fields, or invalid async parameters',
        'Check JSON, model, messages, store, and related fields.'
      ],
      [
        '402/400',
        'Insufficient balance or failed credit check',
        'Recharge or ask an admin to adjust the credit policy.'
      ],
      [
        '404/503',
        'No available channel for the model or upstream unavailable',
        'Check channel models, key health, and provider availability.'
      ],
      [
        '429/5xx',
        'Upstream rate limit, timeout, or failure',
        'Retry later or add/switch upstream channels in the admin console.'
      ]
    ],
    billingIntro:
      'NeoGate estimates credit before forwarding and settles by actual tokens and configured prices after the response completes.',
    billingItems: [
      [
        'Text generation',
        'Usage records include input, output, cache read, and cache write tokens. Prices come from channel prices and selling policy.'
      ],
      [
        'Streaming',
        'Usage is settled after the stream ends. If the stream fails, known usage and the failure summary are recorded.'
      ],
      [
        'Async and batch tasks',
        'Tasks are tracked after creation and settled when they reach a terminal state.'
      ],
      [
        'Failed requests',
        'Requests rejected before forwarding usually do not spend upstream quota, but failure records are kept for diagnostics.'
      ]
    ]
  }
})
</script>

<template>
  <div class="docs-page interfaces-page">
    <header class="home-header docs-header">
      <RouterLink class="home-brand" to="/" :aria-label="t('appName')">
        <img class="home-brand-logo" src="/logo.svg" :alt="t('appName')" />
      </RouterLink>
      <nav class="home-nav" :aria-label="t('appName')">
        <RouterLink to="/">{{ t('home') }}</RouterLink>
        <RouterLink to="/docs">{{ t('docs') }}</RouterLink>
        <RouterLink to="/interfaces">{{ t('interfaces') }}</RouterLink>
      </nav>
      <div class="home-header-actions">
        <LocaleToggleButton class="home-language-button" />
        <a
          class="github-link"
          :href="githubUrl"
          target="_blank"
          rel="noreferrer"
          :aria-label="t('github')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M12 2C6.48 2 2 6.58 2 12.26c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49 0-.24-.01-.88-.01-1.73-2.78.62-3.37-1.38-3.37-1.38-.45-1.19-1.11-1.5-1.11-1.5-.91-.64.07-.63.07-.63 1 .07 1.53 1.06 1.53 1.06.9 1.57 2.35 1.12 2.93.86.09-.67.35-1.12.63-1.38-2.22-.26-4.56-1.14-4.56-5.07 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.33 9.33 0 0 1 12 6.98c.85 0 1.7.12 2.5.34 1.9-1.33 2.74-1.05 2.74-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.94-2.34 4.81-4.57 5.07.36.32.68.95.68 1.92 0 1.38-.01 2.49-.01 2.83 0 .27.18.59.69.49A10.15 10.15 0 0 0 22 12.26C22 6.58 17.52 2 12 2Z"
            />
          </svg>
        </a>
        <RouterLink
          v-if="auth.isAuthed"
          class="home-login-link home-account-link"
          :to="dashboardLink"
          :aria-label="t('admin')"
        >
          <el-icon><UserFilled /></el-icon>
        </RouterLink>
        <RouterLink v-else class="home-login-link" to="/login">{{ t('signIn') }}</RouterLink>
      </div>
    </header>

    <main class="docs-main">
      <section class="docs-hero">
        <h1>{{ content.title }}</h1>
        <p>{{ content.subtitle }}</p>
      </section>

      <div class="docs-layout">
        <aside class="docs-sidebar">
          <h2>{{ content.menuTitle }}</h2>
          <nav>
            <a
              v-for="[id, label, , level] in content.menu"
              :key="id"
              :class="{ 'docs-sidebar-sub-link': level === 'sub' }"
              href=""
              @click.prevent="scrollToSection(id)"
            >
              {{ label }}
            </a>
          </nav>
        </aside>

        <div class="docs-content">
          <section id="before-start" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[0][2] }}</h2>
              <p>{{ content.beforeIntro }}</p>
            </div>
            <div class="docs-feature-grid">
              <article
                v-for="[title, baseUrl, text] in content.protocolCards"
                :key="title"
                class="docs-feature"
              >
                <h3>{{ title }}</h3>
                <code>{{ baseUrl }}</code>
                <p>{{ text }}</p>
              </article>
            </div>
            <div class="docs-check-list">
              <article
                v-for="[title, text] in content.beforeItems"
                :key="title"
                class="docs-check-item"
              >
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>

          <section id="openai" class="docs-section">
            <div class="docs-section-heading">
              <h2>2. {{ content.menu[1][1] }}</h2>
              <p>{{ content.openAiIntro }}</p>
            </div>
            <div class="interface-meta-grid">
              <article v-for="[label, value] in content.openAiAuthItems" :key="label">
                <span>{{ label }}</span>
                <code>{{ value }}</code>
              </article>
            </div>
            <div class="interface-table-wrap">
              <table class="interface-table">
                <thead>
                  <tr>
                    <th v-for="header in content.endpointHeaders" :key="header">{{ header }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="[name, method, path, params, status] in content.openAiEndpoints"
                    :key="`${name}-${method}-${path}`"
                  >
                    <td>{{ name }}</td>
                    <td>
                      <span class="interface-method">{{ method }}</span>
                    </td>
                    <td>
                      <code>{{ path }}</code>
                    </td>
                    <td>{{ params }}</td>
                    <td>
                      <span
                        v-if="status === '已支持' || status === 'Supported'"
                        class="interface-status"
                      >
                        {{ status }}
                      </span>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
            <div class="docs-guide-flow">
              <section id="openai-quick-start" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[2][2] }}</h2>
                </div>
                <article class="docs-step-card">
                  <h3>curl</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiQuickStart)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiQuickStart }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-text" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[3][2] }}</h2>
                  <p>{{ content.openAiText }}</p>
                </div>
                <article class="docs-step-card">
                  <h3>Responses</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponses)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponses }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-stream" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[4][2] }}</h2>
                  <p>{{ content.streamText }}</p>
                </div>
                <article class="docs-step-card">
                  <h3>Chat Completions</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiStream)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiStream }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-async" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[5][2] }}</h2>
                  <p>{{ content.openAiAsync }}</p>
                </div>
                <article class="docs-step-card">
                  <h3>Retrieve Response</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseRetrieve)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseRetrieve }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Cancel Response</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseCancel)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseCancel }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-images" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[6][2] }}</h2>
                  <p>{{ content.openAiImage }}</p>
                </div>
              </section>

              <section id="openai-models" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[7][2] }}</h2>
                </div>
                <article class="docs-step-card">
                  <h3>Models</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiModels)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiModels }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-sdk" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[8][2] }}</h2>
                </div>
                <article class="docs-step-card">
                  <h3>Python</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiPython)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiPython }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Node.js</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiNode)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiNode }}</code></pre>
                  </div>
                </article>
              </section>
            </div>
          </section>

          <section id="anthropic" class="docs-section">
            <div class="docs-section-heading">
              <h2>3. {{ content.menu[9][1] }}</h2>
              <p>{{ content.anthropicIntro }}</p>
            </div>
            <div class="interface-meta-grid">
              <article v-for="[label, value] in content.anthropicAuthItems" :key="label">
                <span>{{ label }}</span>
                <code>{{ value }}</code>
              </article>
            </div>
            <div class="interface-table-wrap">
              <table class="interface-table">
                <thead>
                  <tr>
                    <th v-for="header in content.endpointHeaders" :key="header">{{ header }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="[name, method, path, params, status] in content.anthropicEndpoints"
                    :key="`${name}-${method}-${path}`"
                  >
                    <td>{{ name }}</td>
                    <td>
                      <span class="interface-method">{{ method }}</span>
                    </td>
                    <td>
                      <code>{{ path }}</code>
                    </td>
                    <td>{{ params }}</td>
                    <td>
                      <span
                        v-if="status === '已支持' || status === 'Supported'"
                        class="interface-status"
                      >
                        {{ status }}
                      </span>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
            <div class="docs-guide-flow">
              <section id="anthropic-quick-start" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[10][2] }}</h2>
                </div>
                <article class="docs-step-card">
                  <h3>curl</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(anthropicQuickStart)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ anthropicQuickStart }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="anthropic-text" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[11][2] }}</h2>
                  <p>{{ content.anthropicText }}</p>
                </div>
              </section>

              <section id="anthropic-stream" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[12][2] }}</h2>
                  <p>{{ content.streamText }}</p>
                </div>
                <article class="docs-step-card">
                  <h3>Messages</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(anthropicStream)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ anthropicStream }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="anthropic-batches" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[13][2] }}</h2>
                  <p>{{ content.batchText }}</p>
                </div>
                <article class="docs-step-card">
                  <h3>Message Batches</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(anthropicBatch)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ anthropicBatch }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Retrieve Batch</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(anthropicBatchRetrieve)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ anthropicBatchRetrieve }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Batch Results</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(anthropicBatchResults)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ anthropicBatchResults }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="anthropic-models" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[14][2] }}</h2>
                </div>
                <article class="docs-step-card">
                  <h3>Models</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(anthropicModels)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ anthropicModels }}</code></pre>
                  </div>
                </article>
              </section>
            </div>
          </section>

          <section id="errors" class="docs-section">
            <div class="docs-section-heading">
              <h2>4. {{ content.menu[15][1] }}</h2>
              <p>{{ content.errorIntro }}</p>
            </div>
            <article class="docs-step-card">
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(errorExample)"
                />
                <pre class="docs-code-sample docs-inner-code"><code>{{ errorExample }}</code></pre>
              </div>
            </article>
            <div class="interface-table-wrap">
              <table class="interface-table">
                <thead>
                  <tr>
                    <th v-for="header in content.errorHeaders" :key="header">{{ header }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="[status, reason, handling] in content.errors" :key="status">
                    <td>
                      <span class="interface-method">{{ status }}</span>
                    </td>
                    <td>{{ reason }}</td>
                    <td>{{ handling }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>

          <section id="billing" class="docs-section">
            <div class="docs-section-heading">
              <h2>5. {{ content.menu[16][1] }}</h2>
              <p>{{ content.billingIntro }}</p>
            </div>
            <div class="docs-check-list">
              <article
                v-for="[title, text] in content.billingItems"
                :key="title"
                class="docs-check-item"
              >
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>
        </div>
      </div>
    </main>
  </div>
</template>
