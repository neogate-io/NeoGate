<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy } from '@element-plus/icons-vue'
import PublicHeader from '../../components/PublicHeader.vue'
import { useLocale } from '../../composables/useLocale'
import { useScrollTo, useCopyText } from '../../composables/usePublicPage'

const { locale, t } = useLocale()
const scrollToSection = useScrollTo()
const copyDocText = useCopyText()
const siteOrigin = computed(() => window.location.origin)
const openAiBaseUrl = computed(() => `${siteOrigin.value}/v1`)
const anthropicBaseUrl = computed(() => `${siteOrigin.value}/anthropic`)

function isSupportedStatus(status: string) {
  return status.startsWith('已支持') || status.startsWith('Supported')
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

const openAiResponsesStream = computed(
  () => `curl -N ${openAiBaseUrl.value}/responses \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "连续输出 3 个排查 API 问题的步骤",
    "stream": true
  }'`
)

const openAiResponseBackground = computed(
  () => `curl ${openAiBaseUrl.value}/responses \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "生成一份 500 字的接口迁移说明",
    "background": true,
    "store": true
  }'`
)

const openAiResponseStreamRetrieve = computed(
  () => `curl -N "${openAiBaseUrl.value}/responses/resp_123?stream=true" \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const openAiResponseImageGeneration = computed(
  () => `curl ${openAiBaseUrl.value}/responses \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "生成一张赛博朋克风格的白猫坐在霓虹灯下的图片",
    "tools": [
      {
        "type": "image_generation",
        "model": "gpt-image-2",
        "action": "generate",
        "size": "1024x1024"
      }
    ],
    "background": true,
    "store": true
  }'`
)

const openAiResponseImageEdit = computed(
  () => `IMG_B64="$(base64 < input.png | tr -d '\\n')"

curl ${openAiBaseUrl.value}/responses \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d @- <<JSON
{
  "model": "gpt-5.5",
  "background": true,
  "store": true,
  "tools": [
    {
      "type": "image_generation",
      "model": "gpt-image-2",
      "action": "edit",
      "size": "1024x1024"
    }
  ],
  "input": [
    {
      "role": "user",
      "content": [
        {
          "type": "input_text",
          "text": "基于这张图重新生成：保持主体姿态，改成赛博朋克夜景风格。"
        },
        {
          "type": "input_image",
          "image_url": "data:image/png;base64,$IMG_B64"
        }
      ]
    }
  ]
}
JSON`
)

const openAiImageGeneration = computed(
  () => `curl ${openAiBaseUrl.value}/images/generations \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-image-2",
    "prompt": "A compact glass teapot on a walnut table",
    "size": "1024x1024"
  }'`
)

const openAiImageGenerationStream = computed(
  () => `curl -N ${openAiBaseUrl.value}/images/generations \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-image-2",
    "prompt": "A compact glass teapot on a walnut table",
    "size": "1024x1024",
    "stream": true,
    "partial_images": 2
  }'`
)

const openAiImageEdit = computed(
  () => `curl ${openAiBaseUrl.value}/images/edits \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image=@input.png" \\
  -F "prompt=Add a soft morning light through the window" \\
  -F "size=1024x1024"`
)

const openAiImageEditStream = computed(
  () => `curl -N ${openAiBaseUrl.value}/images/edits \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image=@input.png" \\
  -F "prompt=Add a soft morning light through the window" \\
  -F "size=1024x1024" \\
  -F "stream=true" \\
  -F "partial_images=2"`
)

const openAiImageVariation = computed(
  () => `curl ${openAiBaseUrl.value}/images/variations \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image=@input.png" \\
  -F "size=1024x1024"`
)

const openAiEmbeddings = computed(
  () => `curl ${openAiBaseUrl.value}/embeddings \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "text-embedding-3-small",
    "input": [
      "NeoGate routes OpenAI-compatible API requests.",
      "Embeddings can be used for search and retrieval."
    ],
    "encoding_format": "float"
  }'`
)

const openAiModels = computed(
  () => `curl ${openAiBaseUrl.value}/models \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const openAiPythonInstall = 'pip install openai'

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

const openAiNodeInstall = 'npm install openai'

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
        ['openai-text-async', '文本生成（异步）', '2.3 文本生成（异步）', 'sub'],
        ['openai-images', '图片生成', '2.4 图片生成', 'sub'],
        ['openai-images-async', '图片生成（异步）', '2.5 图片生成（异步）', 'sub'],
        ['openai-embeddings', '向量嵌入', '2.6 向量嵌入', 'sub'],
        ['openai-models', '模型列表', '2.7 模型列表', 'sub'],
        ['openai-sdk', 'SDK 示例', '2.8 SDK 示例', 'sub'],
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
        ['Content-Type', 'application/json；图片上传接口使用 multipart/form-data']
      ],
      openAiEndpoints: [
        ['Models', 'GET', '/v1/models', '-', '已支持'],
        ['Models', 'GET', '/v1/models/{model}', 'model', '已支持'],
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
          '已支持（后台任务）'
        ],
        ['Responses', 'DELETE', '/v1/responses/{response_id}', 'response_id', '开发中'],
        [
          'Responses',
          'POST',
          '/v1/responses/{response_id}/cancel',
          'response_id',
          '已支持（后台任务）'
        ],
        [
          'Responses',
          'GET',
          '/v1/responses/{response_id}/input_items',
          'response_id, limit, after',
          '已支持（后台任务）'
        ],
        [
          'Images',
          'POST',
          '/v1/images/generations',
          'model, prompt, size, quality, n, stream, partial_images',
          '已支持（含流式）'
        ],
        [
          'Images',
          'POST',
          '/v1/images/edits',
          'model, image, prompt, mask, size, n, stream, partial_images',
          '已支持（含流式）'
        ],
        ['Images', 'POST', '/v1/images/variations', 'model, image, size, n', '已支持'],
        [
          'Embeddings',
          'POST',
          '/v1/embeddings',
          'model, input, dimensions, encoding_format',
          '已支持'
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
        ['Moderations', 'POST', '/v1/moderations', 'model, input', '已支持'],
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
        'Chat Completions 与 Responses 均按 OpenAI 官方请求体转发。本节展示同步和流式文本生成。流式输出会以 text/event-stream 持续返回增量内容，适合边生成边展示。Chat Completions 的 stored completion 查询、更新、删除仍在开发中。',
      streamText: '将 stream 设置为 true，响应会以 text/event-stream 形式返回。',
      textAsyncTitle: '文本生成（异步）',
      openAiTextAsync:
        'Responses 后台文本任务按官方 background 参数创建。NeoGate 要求 background=true 时 store 不能为 false；创建后台任务时不支持直接 stream=true，可在查询接口透传 stream=true 恢复流式结果。后台任务只支持 key-backed OpenAI 通道，不走 OpenAI OAuth/Codex 凭证通道。',
      imageTitle: '图片生成',
      openAiImage:
        'Images 支持文生图、图生图/局部编辑和图片变体。生成接口使用 JSON 请求体，编辑接口支持 JSON images 数组或 multipart/form-data 上传图片，变体接口使用 multipart/form-data；流式输出会以 text/event-stream 返回生成过程中的 partial image，适合展示预览进度。',
      imageAsyncTitle: '图片生成（异步）',
      openAiImageAsync:
        '图片后台任务通过 Responses 的 image_generation 工具创建，而不是 Images API 自身的后台任务。可用于文生图异步和图生图异步，创建后使用 Responses 查询、恢复流式结果或取消。',
      requestParamsTitle: '调用参数',
      responseParamsTitle: '返回参数',
      paramFieldHeaders: ['参数', '类型 / 示例', '说明'],
      textRequestParams: [
        [
          'model',
          'string，必填',
          '模型名称，例如 gpt-5.5；会按 NeoGate 的模型权限、渠道选择和计费策略处理。'
        ],
        [
          'messages[]',
          'array，Chat Completions 必填',
          'Chat Completions 对话消息列表，包含 role 与 content。'
        ],
        [
          'input',
          'string | array，Responses 必填',
          'Responses 输入内容，可传字符串、消息数组或多模态内容。'
        ],
        ['instructions', 'string', 'Responses 的系统级指令，适合放置长期行为约束。'],
        ['stream', 'boolean', '设置为 true 时返回 text/event-stream，用于流式输出增量文本。'],
        ['temperature', 'number', '采样温度；数值越高输出越发散，越低越稳定。'],
        ['top_p', 'number', '核采样参数；通常不要和 temperature 同时大幅调整。'],
        ['max_completion_tokens', 'integer', 'Chat Completions 最大生成 Token 数。'],
        ['max_output_tokens', 'integer', 'Responses 最大输出 Token 数。'],
        ['tools / tool_choice', 'array | object', '工具定义和工具选择策略；按官方结构透传。'],
        [
          'response_format / text.format',
          'object',
          '结构化输出设置；Chat Completions 使用 response_format，Responses 使用 text.format。'
        ],
        ['metadata', 'object', '可附加到请求的元数据，便于上游或后续查询识别。']
      ],
      textResponseParams: [
        ['id', 'string', '响应 ID，可用于日志排查或后续关联。'],
        ['object', 'string', '对象类型，例如 chat.completion、chat.completion.chunk 或 response。'],
        ['created / created_at', 'integer', '响应创建时间，Unix 秒级时间戳。'],
        ['model', 'string', '实际返回的模型名称。'],
        ['choices[]', 'array', 'Chat Completions 的候选结果列表。'],
        ['choices[].message.content', 'string', '非流式 Chat Completions 的文本结果。'],
        ['choices[].delta.content', 'string', '流式 Chat Completions 的增量文本片段。'],
        ['choices[].finish_reason', 'string', '生成结束原因，例如 stop、length 或 tool_calls。'],
        ['output[]', 'array', 'Responses 的输出项列表，通常包含 message、tool_call 等类型。'],
        ['output[].content[].text', 'string', 'Responses 文本输出内容。'],
        ['status', 'string', 'Responses 状态，例如 completed、failed 或 incomplete。'],
        ['error', 'object | null', 'Responses 失败时的错误信息；成功时通常为空。'],
        ['usage', 'object | null', 'Token 用量，NeoGate 会用于用量记录和结算。']
      ],
      textAsyncRequestParams: [
        ['model', 'string，必填', 'Responses 主模型，例如 gpt-5.5。'],
        ['input', 'string | array，必填', '后台任务的输入内容。'],
        ['instructions', 'string', '系统级指令，适合放置任务要求或输出约束。'],
        ['background', 'boolean，必填 true', '设置为 true 创建后台 Response。'],
        ['store', 'boolean', 'background=true 时需要保存响应；NeoGate 要求 store 不能为 false。'],
        [
          'stream',
          'boolean',
          '创建后台任务时不要设置为 true；查询接口可追加 ?stream=true 恢复流式结果。'
        ],
        ['max_output_tokens', 'integer', '限制后台任务最多输出的 Token 数。'],
        ['temperature / top_p', 'number', '采样控制参数，影响输出随机性。'],
        ['tools / tool_choice', 'array | object', '工具定义和选择策略；后台任务会按官方字段透传。'],
        ['metadata', 'object', '附加元数据，便于任务追踪和排查。']
      ],
      textAsyncResponseParams: [
        ['id', 'string', 'Response ID，例如 resp_123；用于查询、恢复流式结果或取消。'],
        ['object', '"response"', '返回对象类型。'],
        ['created_at', 'integer', 'Response 创建时间，Unix 秒级时间戳。'],
        ['background', 'boolean', '是否为后台任务。'],
        [
          'status',
          'string',
          '任务状态，例如 queued、in_progress、completed、failed、cancelled 或 incomplete。'
        ],
        ['output[]', 'array', '完成后包含模型输出；文本通常在 message 输出项中。'],
        ['output[].type', 'string', '输出项类型，例如 message、function_call 或 tool 调用结果。'],
        ['output[].content[].type', '"output_text"', '文本内容项类型。'],
        ['output[].content[].text', 'string', '后台任务完成后的文本结果。'],
        ['error', 'object | null', '失败时包含 code 与 message；成功时通常为空。'],
        ['usage', 'object | null', '终态返回的 Token 用量，NeoGate 会用于记录和结算。']
      ],
      imageRequestParams: [
        [
          'model',
          'string，必填',
          '图片模型，例如 gpt-image-2；会按 NeoGate 模型权限和渠道能力转发。'
        ],
        ['prompt', 'string，必填', '用于生成图片的文本描述。'],
        [
          'size',
          'string',
          '图片尺寸，例如 1024x1024、1536x1024、1024x1536 或 auto；可用尺寸以所选模型和上游为准。'
        ],
        ['quality', 'string', '图片质量，例如 auto、low、medium、high；会影响延迟、费用和细节。'],
        ['n', 'integer', '生成图片数量；部分图片模型只支持 1。'],
        ['output_format', 'string', '输出格式，例如 png、jpeg 或 webp。'],
        ['output_compression', 'integer', '输出压缩质量，0-100；通常只对 jpeg/webp 生效。'],
        ['stream', 'boolean', '设置为 true 时返回 text/event-stream，用于接收生成过程中的事件。'],
        ['partial_images', 'integer', '流式图片生成时请求返回的中间预览图数量。']
      ],
      imageResponseParams: [
        ['created', 'integer', '响应创建时间，Unix 秒级时间戳。'],
        ['data[]', 'array', '图片结果列表；每个元素代表一张生成图片。'],
        ['data[].b64_json', 'string', 'Base64 编码的图片内容，客户端可解码保存或展示。'],
        ['data[].url', 'string', '当上游返回 URL 形式图片时透传。'],
        ['data[].revised_prompt', 'string', '上游可能返回的改写后提示词。'],
        ['usage', 'object', '上游返回的图片生成用量信息，NeoGate 会用于用量记录和结算。'],
        [
          'stream event',
          'text/event-stream',
          'stream=true 时返回 partial image、completed、error 等事件。'
        ]
      ],
      imageAsyncRequestParams: [
        [
          'model',
          'string，必填',
          'Responses 主模型，例如 gpt-5.5；负责理解输入并调用 image_generation 工具。'
        ],
        [
          'input',
          'string | array，必填',
          '文生图可直接传字符串；图生图/编辑可传 input_text 与 input_image 组成的消息内容。'
        ],
        ['tools[].type', '"image_generation"，必填', '启用 Responses 的图片生成工具。'],
        ['tools[].model', 'string', '图片模型，例如 gpt-image-2；按上游工具能力转发。'],
        ['tools[].action', 'generate | edit | auto', '控制生成、编辑或由模型自动决定动作。'],
        ['tools[].size', 'string', '图片尺寸，例如 1024x1024、1536x1024、1024x1536 或 auto。'],
        ['tools[].quality', 'string', '图片质量，例如 auto、low、medium、high。'],
        ['tools[].output_format', 'string', '图片输出格式，例如 png、jpeg 或 webp。'],
        [
          'background',
          'boolean',
          '设置为 true 创建后台 Response；NeoGate 的图片异步任务使用该模式。'
        ],
        ['store', 'boolean', 'background=true 时需要保存响应；NeoGate 要求 store 不能为 false。'],
        [
          'stream',
          'boolean',
          '创建后台任务时不要设置为 true；需要流式结果时在查询接口追加 ?stream=true。'
        ]
      ],
      imageAsyncResponseParams: [
        ['id', 'string', 'Response ID，例如 resp_123；用于查询、恢复流式结果或取消。'],
        ['object', '"response"', '返回对象类型。'],
        ['created_at', 'integer', 'Response 创建时间，Unix 秒级时间戳。'],
        ['background', 'boolean', '是否为后台任务。'],
        [
          'status',
          'string',
          '任务状态，例如 queued、in_progress、completed、failed 或 cancelled。'
        ],
        ['output[]', 'array', '完成后包含模型输出；图片结果位于 image_generation_call 项中。'],
        ['output[].type', '"image_generation_call"', '标识该输出项来自图片生成工具。'],
        ['output[].result', 'string', '完成时返回 Base64 图片内容。'],
        ['error', 'object | null', '失败时包含 code 与 message；成功时通常为空。'],
        ['usage', 'object | null', '终态返回的用量信息，NeoGate 会用于记录和结算。']
      ],
      embeddingTitle: '向量嵌入',
      openAiEmbeddings:
        'Embeddings 接口按 OpenAI 官方 JSON 请求体转发，适合 RAG、语义搜索、去重和召回场景。请求中的 model 会走 NeoGate 的模型权限、渠道选择、计费和用量记录。',
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
      ['openai-text-async', 'Text generation async', '2.3 Text generation async', 'sub'],
      ['openai-images', 'Images', '2.4 Images', 'sub'],
      ['openai-images-async', 'Images async', '2.5 Images async', 'sub'],
      ['openai-embeddings', 'Embeddings', '2.6 Embeddings', 'sub'],
      ['openai-models', 'Models', '2.7 Models', 'sub'],
      ['openai-sdk', 'SDK examples', '2.8 SDK examples', 'sub'],
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
      ['Content-Type', 'application/json; image upload APIs use multipart/form-data']
    ],
    openAiEndpoints: [
      ['Models', 'GET', '/v1/models', '-', 'Supported'],
      ['Models', 'GET', '/v1/models/{model}', 'model', 'Supported'],
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
        'Supported (background tasks)'
      ],
      ['Responses', 'DELETE', '/v1/responses/{response_id}', 'response_id', 'In development'],
      [
        'Responses',
        'POST',
        '/v1/responses/{response_id}/cancel',
        'response_id',
        'Supported (background tasks)'
      ],
      [
        'Responses',
        'GET',
        '/v1/responses/{response_id}/input_items',
        'response_id, limit, after',
        'Supported (background tasks)'
      ],
      [
        'Images',
        'POST',
        '/v1/images/generations',
        'model, prompt, size, quality, n, stream, partial_images',
        'Supported (streaming)'
      ],
      [
        'Images',
        'POST',
        '/v1/images/edits',
        'model, image, prompt, mask, size, n, stream, partial_images',
        'Supported (streaming)'
      ],
      ['Images', 'POST', '/v1/images/variations', 'model, image, size, n', 'Supported'],
      [
        'Embeddings',
        'POST',
        '/v1/embeddings',
        'model, input, dimensions, encoding_format',
        'Supported'
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
      ['Moderations', 'POST', '/v1/moderations', 'model, input', 'Supported'],
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
      'Chat Completions and Responses are forwarded with the official OpenAI request body. This section shows synchronous and streaming text generation. Streaming returns incremental content over text/event-stream, which is useful when the UI should render while the model is still generating. Stored Chat Completion retrieve, update, message listing, and delete are still in development.',
    streamText: 'Set stream to true to receive a text/event-stream response.',
    textAsyncTitle: 'Text generation async',
    openAiTextAsync:
      'Background text Responses follow the official background parameter. NeoGate requires store not to be false when background=true. Create-time streaming is not supported for background tasks; retrieve can pass through stream=true to resume streamed results. Background tasks require key-backed OpenAI channels and do not use OpenAI OAuth/Codex credential channels.',
    imageTitle: 'Image generation',
    openAiImage:
      'Images supports text-to-image, image edits, and image variations. Generations use a JSON body; edits support a JSON images array or multipart/form-data image uploads, while variations use multipart/form-data. Streaming returns partial images over text/event-stream, which is useful for showing generation progress.',
    imageAsyncTitle: 'Image generation async',
    openAiImageAsync:
      'Background image tasks are created through the Responses image_generation tool, not through a background mode on the Images API itself. Use it for async text-to-image and image-to-image, then retrieve, resume streaming, or cancel through Responses.',
    requestParamsTitle: 'Request parameters',
    responseParamsTitle: 'Response parameters',
    paramFieldHeaders: ['Parameter', 'Type / example', 'Description'],
    textRequestParams: [
      [
        'model',
        'string, required',
        'Model name, for example gpt-5.5. NeoGate applies model permissions, routing, and billing policy.'
      ],
      [
        'messages[]',
        'array, required for Chat Completions',
        'Chat Completions conversation messages with role and content.'
      ],
      [
        'input',
        'string | array, required for Responses',
        'Responses input content, which can be a string, message array, or multimodal content.'
      ],
      ['instructions', 'string', 'System-level instructions for Responses.'],
      ['stream', 'boolean', 'Set true to receive incremental text over text/event-stream.'],
      [
        'temperature',
        'number',
        'Sampling temperature. Higher values are more varied; lower values are steadier.'
      ],
      [
        'top_p',
        'number',
        'Nucleus sampling value; usually avoid changing it heavily with temperature.'
      ],
      ['max_completion_tokens', 'integer', 'Maximum generated tokens for Chat Completions.'],
      ['max_output_tokens', 'integer', 'Maximum output tokens for Responses.'],
      [
        'tools / tool_choice',
        'array | object',
        'Tool definitions and tool selection policy, passed through.'
      ],
      [
        'response_format / text.format',
        'object',
        'Structured output settings. Chat Completions uses response_format; Responses uses text.format.'
      ],
      [
        'metadata',
        'object',
        'Optional metadata attached to the request for upstream or later lookup.'
      ]
    ],
    textResponseParams: [
      ['id', 'string', 'Response ID for diagnostics or later correlation.'],
      [
        'object',
        'string',
        'Object type, such as chat.completion, chat.completion.chunk, or response.'
      ],
      ['created / created_at', 'integer', 'Unix timestamp for when the response was created.'],
      ['model', 'string', 'Model name returned by the upstream.'],
      ['choices[]', 'array', 'Chat Completions candidate result list.'],
      ['choices[].message.content', 'string', 'Text result for non-streaming Chat Completions.'],
      ['choices[].delta.content', 'string', 'Incremental text for streaming Chat Completions.'],
      [
        'choices[].finish_reason',
        'string',
        'Why generation stopped, such as stop, length, or tool_calls.'
      ],
      [
        'output[]',
        'array',
        'Responses output item list, usually including message, tool_call, and related items.'
      ],
      ['output[].content[].text', 'string', 'Text output content from Responses.'],
      ['status', 'string', 'Responses status, such as completed, failed, or incomplete.'],
      [
        'error',
        'object | null',
        'Responses error information on failure; usually null on success.'
      ],
      ['usage', 'object | null', 'Token usage used by NeoGate for records and settlement.']
    ],
    textAsyncRequestParams: [
      ['model', 'string, required', 'Responses model, for example gpt-5.5.'],
      ['input', 'string | array, required', 'Input content for the background task.'],
      [
        'instructions',
        'string',
        'System-level instructions for task requirements or output constraints.'
      ],
      ['background', 'boolean, required true', 'Set true to create a background Response.'],
      [
        'store',
        'boolean',
        'Required for background responses; NeoGate does not allow store=false.'
      ],
      [
        'stream',
        'boolean',
        'Do not set true during background creation; retrieve with ?stream=true to resume streamed results.'
      ],
      ['max_output_tokens', 'integer', 'Limits the maximum output tokens for the background task.'],
      ['temperature / top_p', 'number', 'Sampling controls that affect output randomness.'],
      [
        'tools / tool_choice',
        'array | object',
        'Tool definitions and selection policy passed through.'
      ],
      ['metadata', 'object', 'Optional metadata for task tracing and diagnostics.']
    ],
    textAsyncResponseParams: [
      [
        'id',
        'string',
        'Response ID, for example resp_123, used to retrieve, resume streaming, or cancel.'
      ],
      ['object', '"response"', 'Object type returned by the Responses API.'],
      ['created_at', 'integer', 'Unix timestamp for when the Response was created.'],
      ['background', 'boolean', 'Whether this is a background task.'],
      [
        'status',
        'string',
        'Task status, such as queued, in_progress, completed, failed, cancelled, or incomplete.'
      ],
      [
        'output[]',
        'array',
        'Completed model output; text usually appears in message output items.'
      ],
      [
        'output[].type',
        'string',
        'Output item type, such as message, function_call, or tool result.'
      ],
      ['output[].content[].type', '"output_text"', 'Text content item type.'],
      ['output[].content[].text', 'string', 'Text result after the background task completes.'],
      ['error', 'object | null', 'On failure, includes code and message; usually null on success.'],
      ['usage', 'object | null', 'Final token usage used by NeoGate for records and settlement.']
    ],
    imageRequestParams: [
      [
        'model',
        'string, required',
        'Image model, for example gpt-image-2. NeoGate still applies model permissions and upstream routing.'
      ],
      ['prompt', 'string, required', 'Text prompt describing the image to generate.'],
      [
        'size',
        'string',
        'Image size, for example 1024x1024, 1536x1024, 1024x1536, or auto. Availability depends on the model and upstream.'
      ],
      ['quality', 'string', 'Image quality, such as auto, low, medium, or high.'],
      ['n', 'integer', 'Number of images to generate; some image models only support 1.'],
      ['output_format', 'string', 'Output image format, such as png, jpeg, or webp.'],
      ['output_compression', 'integer', 'Compression quality from 0-100, usually for jpeg/webp.'],
      ['stream', 'boolean', 'Set true to receive generation events over text/event-stream.'],
      ['partial_images', 'integer', 'Number of partial preview images requested during streaming.']
    ],
    imageResponseParams: [
      ['created', 'integer', 'Unix timestamp for when the response was created.'],
      ['data[]', 'array', 'Image result list; each item represents one generated image.'],
      ['data[].b64_json', 'string', 'Base64-encoded image content for display or storage.'],
      ['data[].url', 'string', 'Passed through when the upstream returns image URLs.'],
      [
        'data[].revised_prompt',
        'string',
        'Prompt revision returned by the upstream when available.'
      ],
      [
        'usage',
        'object',
        'Image generation usage returned by the upstream and used for NeoGate billing records.'
      ],
      [
        'stream event',
        'text/event-stream',
        'With stream=true, events include partial image, completed, and error states.'
      ]
    ],
    imageAsyncRequestParams: [
      [
        'model',
        'string, required',
        'Responses model, for example gpt-5.5. It interprets the input and calls the image_generation tool.'
      ],
      [
        'input',
        'string | array, required',
        'Use a string for text-to-image, or message content with input_text and input_image for image-to-image/editing.'
      ],
      [
        'tools[].type',
        '"image_generation", required',
        'Enables the Responses image generation tool.'
      ],
      [
        'tools[].model',
        'string',
        'Image model, for example gpt-image-2, passed through when supported.'
      ],
      [
        'tools[].action',
        'generate | edit | auto',
        'Controls generation, editing, or automatic action selection.'
      ],
      ['tools[].size', 'string', 'Image size, such as 1024x1024, 1536x1024, 1024x1536, or auto.'],
      ['tools[].quality', 'string', 'Image quality, such as auto, low, medium, or high.'],
      ['tools[].output_format', 'string', 'Output image format, such as png, jpeg, or webp.'],
      [
        'background',
        'boolean',
        'Set true to create a background Response; NeoGate async image tasks use this mode.'
      ],
      [
        'store',
        'boolean',
        'Required for background responses; NeoGate does not allow store=false.'
      ],
      [
        'stream',
        'boolean',
        'Do not set true during background creation; retrieve with ?stream=true to resume streamed results.'
      ]
    ],
    imageAsyncResponseParams: [
      [
        'id',
        'string',
        'Response ID, for example resp_123, used to retrieve, resume streaming, or cancel.'
      ],
      ['object', '"response"', 'Object type returned by the Responses API.'],
      ['created_at', 'integer', 'Unix timestamp for when the Response was created.'],
      ['background', 'boolean', 'Whether this is a background task.'],
      [
        'status',
        'string',
        'Task status, such as queued, in_progress, completed, failed, or cancelled.'
      ],
      [
        'output[]',
        'array',
        'Completed model output; image results appear in image_generation_call items.'
      ],
      [
        'output[].type',
        '"image_generation_call"',
        'Identifies output produced by the image generation tool.'
      ],
      ['output[].result', 'string', 'Base64 image content returned when generation completes.'],
      ['error', 'object | null', 'On failure, includes code and message; usually null on success.'],
      ['usage', 'object | null', 'Final usage data used by NeoGate for records and settlement.']
    ],
    embeddingTitle: 'Embeddings',
    openAiEmbeddings:
      'Embeddings are forwarded with the official OpenAI JSON request body and are useful for RAG, semantic search, deduplication, and retrieval. The requested model still uses NeoGate model permissions, routing, billing, and usage records.',
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
    <PublicHeader header-class="docs-header" />

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
                      <span v-if="isSupportedStatus(status)" class="interface-status">
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
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.requestParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.textRequestParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.responseParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.textResponseParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Responses Create</h3>
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
                <article class="docs-step-card">
                  <h3>Chat Completions Stream</h3>
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
                <article class="docs-step-card">
                  <h3>Responses Stream</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponsesStream)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponsesStream }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-text-async" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[4][2] }}</h2>
                  <p>{{ content.openAiTextAsync }}</p>
                </div>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.requestParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.textAsyncRequestParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.responseParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.textAsyncResponseParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Create Background Response</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseBackground)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseBackground }}</code></pre>
                  </div>
                </article>
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
                  <h3>Retrieve Stream</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseStreamRetrieve)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseStreamRetrieve }}</code></pre>
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
                  <h2>{{ content.menu[5][2] }}</h2>
                  <p>{{ content.openAiImage }}</p>
                </div>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.requestParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.imageRequestParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.responseParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.imageResponseParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Generations</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiImageGeneration)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiImageGeneration }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Generations Stream</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiImageGenerationStream)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiImageGenerationStream }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Edits</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiImageEdit)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiImageEdit }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Edits Stream</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiImageEditStream)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiImageEditStream }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Variations</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiImageVariation)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiImageVariation }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-images-async" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[6][2] }}</h2>
                  <p>{{ content.openAiImageAsync }}</p>
                </div>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.requestParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.imageAsyncRequestParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card docs-params-card">
                  <h3>{{ content.responseParamsTitle }}</h3>
                  <div class="docs-params-table-wrap">
                    <table class="docs-params-table">
                      <thead>
                        <tr>
                          <th v-for="header in content.paramFieldHeaders" :key="header">
                            {{ header }}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr
                          v-for="[name, type, description] in content.imageAsyncResponseParams"
                          :key="name"
                        >
                          <td>
                            <code>{{ name }}</code>
                          </td>
                          <td>{{ type }}</td>
                          <td>{{ description }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Background Text to Image</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseImageGeneration)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseImageGeneration }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>Background Image to Image</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseImageEdit)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseImageEdit }}</code></pre>
                  </div>
                </article>
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
                  <h3>Retrieve Stream</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiResponseStreamRetrieve)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiResponseStreamRetrieve }}</code></pre>
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

              <section id="openai-embeddings" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[7][2] }}</h2>
                  <p>{{ content.openAiEmbeddings }}</p>
                </div>
                <article class="docs-step-card">
                  <h3>Embeddings</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(openAiEmbeddings)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ openAiEmbeddings }}</code></pre>
                  </div>
                </article>
              </section>

              <section id="openai-models" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[8][2] }}</h2>
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
                  <h2>{{ content.menu[9][2] }}</h2>
                </div>
                <article class="docs-step-card">
                  <el-tabs>
                    <el-tab-pane label="Python">
                      <div class="docs-sdk-tab-panel">
                        <div class="docs-copy-block">
                          <el-button
                            :icon="DocumentCopy"
                            text
                            :aria-label="t('copy')"
                            @click="copyDocText(openAiPythonInstall)"
                          />
                          <pre
                            class="docs-code-sample docs-inner-code"
                          ><code>{{ openAiPythonInstall }}</code></pre>
                        </div>
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
                      </div>
                    </el-tab-pane>
                    <el-tab-pane label="Node.js">
                      <div class="docs-sdk-tab-panel">
                        <div class="docs-copy-block">
                          <el-button
                            :icon="DocumentCopy"
                            text
                            :aria-label="t('copy')"
                            @click="copyDocText(openAiNodeInstall)"
                          />
                          <pre
                            class="docs-code-sample docs-inner-code"
                          ><code>{{ openAiNodeInstall }}</code></pre>
                        </div>
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
                      </div>
                    </el-tab-pane>
                  </el-tabs>
                </article>
              </section>
            </div>
          </section>

          <section id="anthropic" class="docs-section">
            <div class="docs-section-heading">
              <h2>3. {{ content.menu[10][1] }}</h2>
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
                      <span v-if="isSupportedStatus(status)" class="interface-status">
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
                  <h2>{{ content.menu[11][2] }}</h2>
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
                  <h2>{{ content.menu[12][2] }}</h2>
                  <p>{{ content.anthropicText }}</p>
                </div>
              </section>

              <section id="anthropic-stream" class="docs-subsection">
                <div class="docs-section-heading docs-subsection-heading">
                  <h2>{{ content.menu[13][2] }}</h2>
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
                  <h2>{{ content.menu[14][2] }}</h2>
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
                  <h2>{{ content.menu[15][2] }}</h2>
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
              <h2>4. {{ content.menu[16][1] }}</h2>
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
              <h2>5. {{ content.menu[17][1] }}</h2>
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
