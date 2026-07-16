<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy } from '@element-plus/icons-vue'
import { useLocale } from '../../../composables/useLocale'
import { useSiteBrand } from '../../../composables/useSiteBrand'
import { useCopyText } from '../../../composables/usePublicPage'
import InterfaceEndpointList from './InterfaceEndpointList.vue'

const { locale, t } = useLocale()
const { siteName } = useSiteBrand()
const copyDocText = useCopyText()
const siteOrigin = computed(() => window.location.origin)

const openAiBaseUrl = computed(() => `${siteOrigin.value}/v1`)

function isSupportedStatus(status: string) {
  return status.startsWith('已支持') || status.startsWith('Supported')
}

function endpointDescription(name: string, method: string, path: string) {
  const isZh = locale.value === 'zh-CN'
  const key = `${method} ${path}`

  const zhDescriptions: Record<string, string> = {
    'GET /v1/models': '列出当前 API Key 可调用的模型。',
    'GET /v1/models/{model}': '查询单个模型详情。',
    'DELETE /v1/models/{model}': '删除或取消可删除模型。',
    'POST /v1/chat/completions': '创建 Chat Completions 文本生成。',
    'GET /v1/chat/completions/{completion_id}': '查询已保存的 Chat Completion。',
    'GET /v1/chat/completions/{completion_id}/messages': '列出已保存对话的消息。',
    'PATCH /v1/chat/completions/{completion_id}': '更新已保存对话的元数据。',
    'DELETE /v1/chat/completions/{completion_id}': '删除已保存的 Chat Completion。',
    'POST /v1/responses': '创建 Responses 文本、多模态或后台任务。',
    'GET /v1/responses/{response_id}': '查询 Response 结果或恢复流式读取。',
    'DELETE /v1/responses/{response_id}': '删除已保存的 Response。',
    'POST /v1/responses/{response_id}/cancel': '取消后台 Response 任务。',
    'GET /v1/responses/{response_id}/input_items': '列出 Response 的输入项。',
    'POST /v1/images/generations': '根据提示词生成图片。',
    'POST /v1/images/edits': '编辑上传图片或进行图生图。',
    'POST /v1/images/variations': '基于输入图片生成变体。',
    'POST /v1/videos': '创建视频生成任务。',
    'GET /v1/videos': '列出视频任务。',
    'GET /v1/videos/{video_id}': '查询视频任务状态。',
    'DELETE /v1/videos/{video_id}': '删除视频任务。',
    'GET /v1/videos/{video_id}/content': '下载生成完成的视频文件。',
    'POST /v1/videos/edits': '编辑已有视频。',
    'POST /v1/videos/extensions': '扩展已有视频时长或内容。',
    'POST /v1/videos/{video_id}/remix': '基于已有视频重新生成版本。',
    'POST /v1/embeddings': '创建文本向量嵌入。',
    'POST /v1/audio/speech': '将文本转换为语音。',
    'POST /v1/audio/transcriptions': '将音频转写为文本。',
    'POST /v1/audio/translations': '将音频翻译为文本。',
    'POST /v1/moderations': '对输入内容进行安全审核。',
    'POST /v1/files': '上传文件资源。',
    'GET /v1/files': '列出已上传文件。',
    'GET /v1/files/{file_id}': '查询文件元数据。',
    'DELETE /v1/files/{file_id}': '删除文件。',
    'GET /v1/files/{file_id}/content': '下载文件内容。',
    'POST /v1/uploads': '创建分片上传会话。',
    'POST /v1/uploads/{upload_id}/parts': '上传一个文件分片。',
    'POST /v1/uploads/{upload_id}/complete': '完成分片上传。',
    'POST /v1/uploads/{upload_id}/cancel': '取消分片上传。',
    'POST /v1/batches': '创建 OpenAI 批量任务。',
    'GET /v1/batches': '列出批量任务。',
    'GET /v1/batches/{batch_id}': '查询批量任务。',
    'POST /v1/batches/{batch_id}/cancel': '取消批量任务。',
    'POST /v1/fine_tuning/jobs': '创建微调任务。',
    'GET /v1/fine_tuning/jobs': '列出微调任务。',
    'GET /v1/fine_tuning/jobs/{fine_tuning_job_id}': '查询微调任务。',
    'POST /v1/fine_tuning/jobs/{fine_tuning_job_id}/cancel': '取消微调任务。',
    'GET /v1/fine_tuning/jobs/{fine_tuning_job_id}/events': '列出微调事件。',
    'GET /v1/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints': '列出微调检查点。',
    'POST /v1/vector_stores': '创建向量库。',
    'GET /v1/vector_stores': '列出向量库。',
    'GET/PATCH/DELETE /v1/vector_stores/{vector_store_id}': '查询、更新或删除向量库。',
    'POST/GET /v1/vector_stores/{vector_store_id}/files': '添加或列出向量库文件。',
    'GET/DELETE /v1/vector_stores/{vector_store_id}/files/{file_id}': '查询或移除向量库文件。',
    'POST /v1/vector_stores/{vector_store_id}/file_batches': '创建向量库文件批处理。',
    'GET/POST /v1/vector_stores/{vector_store_id}/file_batches/{batch_id}':
      '查询或取消文件批处理。',
    'GET /v1/vector_stores/{vector_store_id}/file_batches/{batch_id}/files': '列出批处理中的文件。',
    'POST /v1/vector_stores/{vector_store_id}/search': '检索向量库内容。',
    'POST /v1/threads': '创建 Assistants 线程。',
    'GET/PATCH/DELETE /v1/threads/{thread_id}': '查询、更新或删除线程。',
    'POST/GET /v1/threads/{thread_id}/messages': '创建或列出线程消息。',
    'GET/PATCH/DELETE /v1/threads/{thread_id}/messages/{message_id}': '查询、更新或删除线程消息。',
    'POST/GET /v1/threads/{thread_id}/runs': '创建或列出线程运行。',
    'GET/PATCH /v1/threads/{thread_id}/runs/{run_id}': '查询或更新线程运行。',
    'POST /v1/threads/{thread_id}/runs/{run_id}/cancel': '取消线程运行。',
    'POST /v1/threads/{thread_id}/runs/{run_id}/submit_tool_outputs': '提交工具调用结果。',
    'POST /v1/realtime/sessions': '创建实时语音/多模态会话。',
    'POST /v1/realtime/transcription_sessions': '创建实时转写会话。',
    'POST/GET /v1/evals': '创建或列出评测。',
    'GET/PATCH/DELETE /v1/evals/{eval_id}': '查询、更新或删除评测。',
    'POST/GET /v1/evals/{eval_id}/runs': '创建或列出评测运行。',
    'GET/DELETE /v1/evals/{eval_id}/runs/{run_id}': '查询或删除评测运行。'
  }

  const enDescriptions: Record<string, string> = {
    'GET /v1/models': 'List models callable by the current API key.',
    'GET /v1/models/{model}': 'Retrieve a single model.',
    'DELETE /v1/models/{model}': 'Delete or cancel a deletable model.',
    'POST /v1/chat/completions': 'Create Chat Completions text generation.',
    'POST /v1/responses': 'Create Responses text, multimodal, or background tasks.',
    'POST /v1/images/generations': 'Generate images from prompts.',
    'POST /v1/images/edits': 'Edit uploaded images or image inputs.',
    'POST /v1/images/variations': 'Create variations from an input image.',
    'POST /v1/videos': 'Create a video generation task.',
    'GET /v1/videos/{video_id}': 'Retrieve video task status.',
    'GET /v1/videos/{video_id}/content': 'Download completed video content.',
    'POST /v1/embeddings': 'Create text embeddings.',
    'POST /v1/moderations': 'Moderate input content.'
  }

  const fallback = isZh ? `${name} 接口功能。` : `${name} endpoint operation.`
  return (isZh ? zhDescriptions : enDescriptions)[key] ?? fallback
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
    "image_format": "url",
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
  () => `IMG_B64="$(base64 < input.jpg | tr -d '\\n')"

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
      "size": "1024x1536",
      "background": "transparent",
      "output_format": "png"
    }
  ],
  "input": [
    {
      "role": "user",
      "content": [
        {
          "type": "input_text",
          "text": "Cut out the dog from this image."
        },
        {
          "type": "input_image",
          "image_url": "data:image/jpeg;base64,$IMG_B64"
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
  -F "image[]=@input.png" \\
  -F "prompt=Add a soft morning light through the window" \\
  -F "size=1024x1024"`
)

const openAiImageEditStream = computed(
  () => `curl -N ${openAiBaseUrl.value}/images/edits \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image[]=@input.png" \\
  -F "prompt=Add a soft morning light through the window" \\
  -F "size=1024x1024" \\
  -F "stream=true" \\
  -F "partial_images=2"`
)

const openAiImageVariation = computed(
  () => `curl ${openAiBaseUrl.value}/images/variations \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -F "model=dall-e-2" \\
  -F "image=@input.png" \\
  -F "size=1024x1024"`
)

const openAiVideoCreate = computed(
  () => `curl ${openAiBaseUrl.value}/videos \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "sora-2",
    "prompt": "Wide tracking shot of a teal coupe driving through a desert highway",
    "size": "1280x720",
    "seconds": "8"
  }'`
)

const openAiVideoCreateWithReference = computed(
  () => `curl ${openAiBaseUrl.value}/videos \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  -F "model=sora-2" \\
  -F "prompt=A lantern-lit street slowly filling with rain reflections" \\
  -F "size=720x1280" \\
  -F "seconds=4" \\
  -F "input_reference=@reference.png;type=image/png"`
)

const openAiVideoRetrieve = computed(
  () => `curl ${openAiBaseUrl.value}/videos/video_123 \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const openAiVideoContent = computed(
  () => `curl ${openAiBaseUrl.value}/videos/video_123/content \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY" \\
  --output video.mp4`
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

const openAiResponseRetrieve = computed(
  () => `curl ${openAiBaseUrl.value}/responses/resp_123 \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const openAiResponseCancel = computed(
  () => `curl ${openAiBaseUrl.value}/responses/resp_123/cancel \\
  -X POST \\
  -H "Authorization: Bearer YOUR_NEOGATE_API_KEY"`
)

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      openAiTitle: '2. OpenAI 兼容接口',
      openAiQuickStartTitle: '2.1 快速开始',
      openAiTextTitle: '2.2 文本生成',
      openAiTextAsyncTitle: '2.3 文本生成（异步）',
      openAiImageTitle: '2.4 图片生成',
      openAiImageAsyncTitle: '2.5 图片生成（异步）',
      openAiVideoTitle: '2.6 视频生成',
      openAiEmbeddingsTitle: '2.7 向量嵌入',
      openAiModelsTitle: '2.8 模型列表',
      openAiSdkTitle: '2.9 SDK 示例',
      urlPathsTitle: 'URL 路径',
      openAiTextPaths: [
        ['POST', `${openAiBaseUrl.value}/chat/completions`, 'Chat Completions'],
        ['POST', `${openAiBaseUrl.value}/responses`, 'Responses']
      ],
      openAiTextAsyncPaths: [
        ['POST', `${openAiBaseUrl.value}/responses`, '创建后台任务'],
        ['GET', `${openAiBaseUrl.value}/responses/{response_id}`, '查询任务结果'],
        ['GET', `${openAiBaseUrl.value}/responses/{response_id}?stream=true`, '恢复流式读取'],
        ['POST', `${openAiBaseUrl.value}/responses/{response_id}/cancel`, '取消任务']
      ],
      openAiImagePaths: [
        ['POST', `${openAiBaseUrl.value}/images/generations`, '图片生成'],
        ['POST', `${openAiBaseUrl.value}/images/edits`, '图片编辑'],
        ['POST', `${openAiBaseUrl.value}/images/variations`, '图片变体']
      ],
      openAiImageAsyncPaths: [
        ['POST', `${openAiBaseUrl.value}/responses`, '通过 Responses 创建图片后台任务'],
        ['GET', `${openAiBaseUrl.value}/responses/{response_id}`, '查询任务结果'],
        ['GET', `${openAiBaseUrl.value}/responses/{response_id}?stream=true`, '恢复流式读取'],
        ['POST', `${openAiBaseUrl.value}/responses/{response_id}/cancel`, '取消任务']
      ],
      openAiVideoPaths: [
        ['POST', `${openAiBaseUrl.value}/videos`, '创建视频任务'],
        ['GET', `${openAiBaseUrl.value}/videos/{video_id}`, '查询视频任务'],
        ['GET', `${openAiBaseUrl.value}/videos/{video_id}/content`, '下载视频内容']
      ],
      openAiEmbeddingsPaths: [['POST', `${openAiBaseUrl.value}/embeddings`, '创建向量嵌入']],
      openAiModelsPaths: [
        ['GET', `${openAiBaseUrl.value}/models`, '模型列表'],
        ['GET', `${openAiBaseUrl.value}/models/{model}`, '模型详情']
      ],
      openAiTextInterfaces: [
        {
          title: 'Chat Completions',
          method: 'POST',
          path: `${openAiBaseUrl.value}/chat/completions`,
          description: '使用 messages 对话格式创建同步或流式文本生成。',
          requestParams: [
            ['model', 'string，必填', '模型名称，例如 gpt-5.5。'],
            ['messages', 'array，必填', '对话消息列表，包含 role 与 content。'],
            ['stream', 'boolean', '设置为 true 时返回流式增量。'],
            ['tools / tool_choice', 'array | object', '工具定义和工具选择策略。']
          ],
          responseFields: [
            ['id', 'string', '响应 ID。'],
            ['choices[]', 'array', '候选结果列表。'],
            ['choices[].message.content', 'string', '非流式文本结果。'],
            ['choices[].delta.content', 'string', '流式增量文本片段。'],
            ['usage', 'object | null', 'Token 用量。']
          ]
        },
        {
          title: 'Responses Create',
          method: 'POST',
          path: `${openAiBaseUrl.value}/responses`,
          description: '使用 input 格式创建 Responses 文本生成。',
          requestParams: [
            ['model', 'string，必填', '模型名称，例如 gpt-5.5。'],
            ['input', 'string | array，必填', '输入内容，可传字符串、消息数组或多模态内容。'],
            ['instructions', 'string', '系统级指令。'],
            ['stream', 'boolean', '设置为 true 时返回流式事件。'],
            ['text.format', 'object', '结构化输出设置。']
          ],
          responseFields: [
            ['id', 'string', 'Response ID。'],
            ['status', 'string', '响应状态，例如 completed、failed。'],
            ['output[]', 'array', 'Responses 输出项列表。'],
            ['output[].content[].text', 'string', '文本输出内容。'],
            ['usage', 'object | null', 'Token 用量。']
          ]
        }
      ],
      openAiTextAsyncInterfaces: [
        {
          title: 'Create Background Response',
          method: 'POST',
          path: `${openAiBaseUrl.value}/responses`,
          description: '创建后台文本任务。',
          requestParams: [
            ['model', 'string，必填', 'Responses 主模型。'],
            ['input', 'string | array，必填', '后台任务输入内容。'],
            ['background', 'boolean，必填 true', '设置为 true 创建后台任务。'],
            ['store', 'boolean', '后台任务需要保存响应，不能为 false。']
          ],
          responseFields: [
            ['id', 'string', 'Response ID，用于查询或取消。'],
            ['status', 'string', 'queued、in_progress、completed、failed 等状态。'],
            ['background', 'boolean', '是否为后台任务。']
          ]
        },
        {
          title: 'Retrieve / Stream / Cancel Response',
          method: 'GET / POST',
          path: `${openAiBaseUrl.value}/responses/{response_id}`,
          description: '查询后台任务、追加 ?stream=true 恢复流式读取，或调用 /cancel 取消任务。',
          requestParams: [
            ['response_id', 'string，必填', '创建后台任务返回的 Response ID。'],
            ['stream', 'boolean', '查询时追加 stream=true 可恢复流式读取。']
          ],
          responseFields: [
            ['status', 'string', '任务当前状态。'],
            ['output[]', 'array', '任务完成后的输出内容。'],
            ['error', 'object | null', '失败时的错误信息。'],
            ['usage', 'object | null', '终态用量。']
          ]
        }
      ],
      openAiImageInterfaces: [
        {
          title: 'Images Generations',
          method: 'POST',
          path: `${openAiBaseUrl.value}/images/generations`,
          description: '根据 prompt 生成图片。',
          requestParams: [
            ['model', 'string，必填', '图片模型，例如 gpt-image-2。'],
            ['prompt', 'string，必填', '图片描述。'],
            ['size', 'string', '图片尺寸。'],
            ['stream', 'boolean', '是否返回流式图片事件。']
          ],
          responseFields: [
            ['created', 'integer', '创建时间。'],
            ['data[]', 'array', '图片结果列表。'],
            ['data[].b64_json / url', 'string', '图片内容或 URL。'],
            ['usage', 'object', '图片生成用量。']
          ]
        },
        {
          title: 'Images Edits',
          method: 'POST',
          path: `${openAiBaseUrl.value}/images/edits`,
          description:
            '编辑或扩展图片。multipart 使用 image 或 image[] 上传文件；JSON 使用 images 数组传入 image_url 或 file_id。',
          requestParams: [
            ['model', 'string，必填', 'GPT Image 模型。'],
            ['image / image[]', 'file | file[]', 'multipart/form-data 的输入图片。'],
            ['images', 'array', 'JSON 请求的输入图片；元素使用 image_url 或 file_id。'],
            ['prompt', 'string，必填', '编辑提示词。'],
            [
              'mask',
              'file | object',
              '可选编辑蒙版；multipart 使用文件，JSON 使用 image_url 或 file_id。'
            ],
            ['size', 'string', '输出尺寸。']
          ],
          responseFields: [
            ['created', 'integer', '创建时间。'],
            ['data[]', 'array', '编辑图片结果。'],
            ['data[].b64_json / url', 'string', '图片内容或 URL。']
          ]
        },
        {
          title: 'Images Variations',
          method: 'POST',
          path: `${openAiBaseUrl.value}/images/variations`,
          description: '基于输入图片生成变体；官方接口仅支持 dall-e-2。',
          requestParams: [
            ['model', '"dall-e-2"，必填', '变体接口仅支持 dall-e-2。'],
            ['image', 'file，必填', '输入图片。'],
            ['n', 'integer', '生成的变体数量。'],
            ['size', 'string', '输出尺寸。']
          ],
          responseFields: [
            ['created', 'integer', '创建时间。'],
            ['data[]', 'array', '变体图片结果。'],
            ['data[].b64_json / url', 'string', '图片内容或 URL。']
          ]
        }
      ],
      openAiImageAsyncInterfaces: [
        {
          title: 'Responses Image Task',
          method: 'POST',
          path: `${openAiBaseUrl.value}/responses`,
          description: '通过 Responses 的 image_generation 工具创建图片后台任务；编辑时传入 input_image 并设置 action=edit。',
          requestParams: [
            ['model', 'string，必填', 'Responses 主模型。'],
            ['input', 'string | array，必填', '文生图或图生图输入。'],
            ['tools[].type', '"image_generation"', '启用图片生成工具。'],
            ['tools[].action', 'generate | edit | auto', '编辑输入图片时设置为 edit。'],
            ['background', 'boolean，必填 true', '创建后台任务。'],
            ['image_format', 'base64 | url | both', 'NeoGate 扩展，控制图片结果格式。']
          ],
          responseFields: [
            ['id', 'string', 'Response ID。'],
            ['status', 'string', '后台任务状态。'],
            ['output[].result', 'string', 'Base64 图片内容。'],
            ['output[].url', 'string', 'URL 图片结果。']
          ]
        }
      ],
      openAiVideoInterfaces: [
        {
          title: 'Videos Create',
          method: 'POST',
          path: `${openAiBaseUrl.value}/videos`,
          description: '创建视频生成任务。',
          requestParams: [
            ['model', 'string，必填', '视频模型，例如 sora-2。'],
            ['prompt', 'string，必填', '视频内容描述。'],
            ['input_reference', 'file | object', '可选参考图。'],
            ['size', 'string', '视频尺寸。'],
            ['seconds', 'string | number', '视频时长。']
          ],
          responseFields: [
            ['id', 'string', '视频任务 ID。'],
            ['status', 'string', '任务状态。'],
            ['progress', 'number', '任务进度。'],
            ['error', 'object | null', '失败信息。']
          ]
        },
        {
          title: 'Videos Retrieve / Content',
          method: 'GET',
          path: `${openAiBaseUrl.value}/videos/{video_id}`,
          description: '查询视频任务；使用 /content 下载完成后的 MP4。',
          requestParams: [['video_id', 'string，必填', '视频任务 ID。']],
          responseFields: [
            ['status', 'string', '任务状态。'],
            ['completed_at', 'integer | null', '完成时间。'],
            ['content', 'binary', '/content 返回视频文件。']
          ]
        }
      ],
      openAiEmbeddingsInterfaces: [
        {
          title: 'Embeddings',
          method: 'POST',
          path: `${openAiBaseUrl.value}/embeddings`,
          description: '创建文本向量。',
          requestParams: [
            ['model', 'string，必填', '向量模型。'],
            ['input', 'string | array，必填', '待向量化文本。'],
            ['encoding_format', 'string', '返回格式，例如 float。']
          ],
          responseFields: [
            ['data[]', 'array', '向量结果列表。'],
            ['data[].embedding', 'number[]', '向量数组。'],
            ['usage', 'object', 'Token 用量。']
          ]
        }
      ],
      openAiModelsInterfaces: [
        {
          title: 'Models',
          method: 'GET',
          path: `${openAiBaseUrl.value}/models`,
          description: '获取当前 API Key 可调用的模型列表。',
          requestParams: [],
          responseFields: [
            ['data[]', 'array', '模型列表。'],
            ['data[].id', 'string', '模型 ID。'],
            ['data[].owned_by', 'string', '模型来源。']
          ]
        }
      ],
      videoWorkflowTitle: '视频生成流程',
      videoWorkflowItems: [
        [
          '创建任务',
          '调用 /v1/videos 创建视频任务，可选择纯文本 prompt 或上传 input_reference 参考图。'
        ],
        ['查询状态', '通过 /v1/videos/{video_id} 查询任务状态、进度和失败原因。'],
        ['下载内容', '任务完成后调用 /v1/videos/{video_id}/content 下载 MP4 文件。']
      ],
      videoNotes: [
        [
          '参考图上传',
          '带参考图的视频创建使用 multipart/form-data，input_reference 字段传入图片文件。'
        ],
        [
          '任务状态',
          '返回 status 为 queued、in_progress、completed 或 failed；失败时查看 error 字段。'
        ]
      ],
      endpointHeaders: ['模块', '方法', '官方路径', '接口说明', '状态'],
      openAiIntro: `OpenAI 兼容接口统一使用 Bearer Token 认证。Base URL 填写 ${siteName.value} 的 /v1 地址。下表按 OpenAI 官方 API reference 列出接口族；其他接口会在状态列说明当前支持情况。`,
      openAiAuthItems: [
        ['Base URL', openAiBaseUrl.value],
        ['认证头', 'Authorization: Bearer YOUR_NEOGATE_API_KEY'],
        ['Content-Type', 'application/json；图片和视频上传接口使用 multipart/form-data']
      ],
      openAiEndpoints: [
        ['Models', 'GET', '/v1/models', '-', '已支持'],
        ['Models', 'GET', '/v1/models/{model}', 'model', '已支持'],
        ['Models', 'DELETE', '/v1/models/{model}', 'model', '暂未支持'],
        ['Chat Completions', 'POST', '/v1/chat/completions', 'model, messages, stream', '已支持'],
        [
          'Chat Completions',
          'GET',
          '/v1/chat/completions/{completion_id}',
          'completion_id',
          '暂未支持'
        ],
        [
          'Chat Completions',
          'GET',
          '/v1/chat/completions/{completion_id}/messages',
          'completion_id',
          '暂未支持'
        ],
        [
          'Chat Completions',
          'PATCH',
          '/v1/chat/completions/{completion_id}',
          'completion_id, metadata',
          '暂未支持'
        ],
        [
          'Chat Completions',
          'DELETE',
          '/v1/chat/completions/{completion_id}',
          'completion_id',
          '暂未支持'
        ],
        ['Responses', 'POST', '/v1/responses', 'model, input, stream, background, store', '已支持'],
        [
          'Responses',
          'GET',
          '/v1/responses/{response_id}',
          'response_id, stream, starting_after',
          '已支持（后台任务）'
        ],
        ['Responses', 'DELETE', '/v1/responses/{response_id}', 'response_id', '暂未支持'],
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
          'model, image/image[] or images, prompt, mask, size, n, stream, partial_images',
          '已支持（含流式）'
        ],
        ['Images', 'POST', '/v1/images/variations', 'model=dall-e-2, image, size, n', '已支持'],
        ['Videos', 'POST', '/v1/videos', 'model, prompt, input_reference, size, seconds', '已支持'],
        ['Videos', 'GET', '/v1/videos', 'limit, after, order', '暂未支持'],
        ['Videos', 'GET', '/v1/videos/{video_id}', 'video_id', '已支持'],
        ['Videos', 'DELETE', '/v1/videos/{video_id}', 'video_id', '暂未支持'],
        ['Videos', 'GET', '/v1/videos/{video_id}/content', 'video_id', '已支持'],
        ['Videos', 'POST', '/v1/videos/edits', 'prompt, video.id', '暂未支持'],
        ['Videos', 'POST', '/v1/videos/extensions', 'prompt, seconds, video.id', '暂未支持'],
        ['Videos', 'POST', '/v1/videos/{video_id}/remix', 'video_id, prompt', '暂未支持'],
        [
          'Embeddings',
          'POST',
          '/v1/embeddings',
          'model, input, dimensions, encoding_format',
          '已支持'
        ],
        ['Audio', 'POST', '/v1/audio/speech', 'model, input, voice, response_format', '暂未支持'],
        [
          'Audio',
          'POST',
          '/v1/audio/transcriptions',
          'model, file, language, response_format',
          '暂未支持'
        ],
        ['Audio', 'POST', '/v1/audio/translations', 'model, file, response_format', '暂未支持'],
        ['Moderations', 'POST', '/v1/moderations', 'model, input', '已支持'],
        ['Files', 'POST', '/v1/files', 'file, purpose', '暂未支持'],
        ['Files', 'GET', '/v1/files', 'purpose, limit, after', '暂未支持'],
        ['Files', 'GET', '/v1/files/{file_id}', 'file_id', '暂未支持'],
        ['Files', 'DELETE', '/v1/files/{file_id}', 'file_id', '暂未支持'],
        ['Files', 'GET', '/v1/files/{file_id}/content', 'file_id', '暂未支持'],
        ['Uploads', 'POST', '/v1/uploads', 'purpose, filename, bytes, mime_type', '暂未支持'],
        ['Uploads', 'POST', '/v1/uploads/{upload_id}/parts', 'upload_id, data', '暂未支持'],
        ['Uploads', 'POST', '/v1/uploads/{upload_id}/complete', 'upload_id, part_ids', '暂未支持'],
        ['Uploads', 'POST', '/v1/uploads/{upload_id}/cancel', 'upload_id', '暂未支持'],
        [
          'Batches',
          'POST',
          '/v1/batches',
          'input_file_id, endpoint, completion_window',
          '暂未支持'
        ],
        ['Batches', 'GET', '/v1/batches', 'limit, after', '暂未支持'],
        ['Batches', 'GET', '/v1/batches/{batch_id}', 'batch_id', '暂未支持'],
        ['Batches', 'POST', '/v1/batches/{batch_id}/cancel', 'batch_id', '暂未支持'],
        [
          'Fine-tuning',
          'POST',
          '/v1/fine_tuning/jobs',
          'model, training_file, validation_file, hyperparameters',
          '暂未支持'
        ],
        ['Fine-tuning', 'GET', '/v1/fine_tuning/jobs', 'limit, after', '暂未支持'],
        [
          'Fine-tuning',
          'GET',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}',
          'fine_tuning_job_id',
          '暂未支持'
        ],
        [
          'Fine-tuning',
          'POST',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}/cancel',
          'fine_tuning_job_id',
          '暂未支持'
        ],
        [
          'Fine-tuning',
          'GET',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}/events',
          'fine_tuning_job_id, limit, after',
          '暂未支持'
        ],
        [
          'Fine-tuning',
          'GET',
          '/v1/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints',
          'fine_tuning_job_id, limit, after',
          '暂未支持'
        ],
        ['Vector Stores', 'POST', '/v1/vector_stores', 'name, file_ids, expires_after', '暂未支持'],
        ['Vector Stores', 'GET', '/v1/vector_stores', 'limit, after, before', '暂未支持'],
        [
          'Vector Stores',
          'GET/PATCH/DELETE',
          '/v1/vector_stores/{vector_store_id}',
          'vector_store_id',
          '暂未支持'
        ],
        [
          'Vector Store Files',
          'POST/GET',
          '/v1/vector_stores/{vector_store_id}/files',
          'vector_store_id, file_id',
          '暂未支持'
        ],
        [
          'Vector Store Files',
          'GET/DELETE',
          '/v1/vector_stores/{vector_store_id}/files/{file_id}',
          'vector_store_id, file_id',
          '暂未支持'
        ],
        [
          'Vector Store File Batches',
          'POST/GET',
          '/v1/vector_stores/{vector_store_id}/file_batches',
          'vector_store_id, file_ids',
          '暂未支持'
        ],
        [
          'Vector Store File Batches',
          'GET/POST',
          '/v1/vector_stores/{vector_store_id}/file_batches/{batch_id}',
          'vector_store_id, batch_id',
          '暂未支持'
        ],
        [
          'Assistants',
          'POST/GET',
          '/v1/assistants',
          'model, instructions, tools, metadata',
          '暂未支持'
        ],
        [
          'Assistants',
          'GET/PATCH/DELETE',
          '/v1/assistants/{assistant_id}',
          'assistant_id',
          '暂未支持'
        ],
        ['Threads', 'POST', '/v1/threads', 'messages, metadata, tool_resources', '暂未支持'],
        ['Threads', 'GET/PATCH/DELETE', '/v1/threads/{thread_id}', 'thread_id', '暂未支持'],
        [
          'Thread Messages',
          'POST/GET',
          '/v1/threads/{thread_id}/messages',
          'thread_id, role, content',
          '暂未支持'
        ],
        [
          'Thread Messages',
          'GET/PATCH/DELETE',
          '/v1/threads/{thread_id}/messages/{message_id}',
          'thread_id, message_id',
          '暂未支持'
        ],
        [
          'Thread Runs',
          'POST/GET',
          '/v1/threads/{thread_id}/runs',
          'thread_id, assistant_id, model',
          '暂未支持'
        ],
        [
          'Thread Runs',
          'GET/PATCH',
          '/v1/threads/{thread_id}/runs/{run_id}',
          'thread_id, run_id',
          '暂未支持'
        ],
        [
          'Thread Runs',
          'POST',
          '/v1/threads/{thread_id}/runs/{run_id}/cancel',
          'thread_id, run_id',
          '暂未支持'
        ],
        [
          'Thread Runs',
          'POST',
          '/v1/threads/{thread_id}/runs/{run_id}/submit_tool_outputs',
          'thread_id, run_id, tool_outputs',
          '暂未支持'
        ],
        [
          'Realtime',
          'POST',
          '/v1/realtime/sessions',
          'model, voice, modalities, instructions',
          '暂未支持'
        ],
        [
          'Realtime',
          'POST',
          '/v1/realtime/transcription_sessions',
          'input_audio_format, input_audio_transcription',
          '暂未支持'
        ],
        [
          'Evals',
          'POST/GET',
          '/v1/evals',
          'name, data_source_config, testing_criteria',
          '暂未支持'
        ],
        ['Evals', 'GET/PATCH/DELETE', '/v1/evals/{eval_id}', 'eval_id', '暂未支持'],
        [
          'Eval Runs',
          'POST/GET',
          '/v1/evals/{eval_id}/runs',
          'eval_id, data_source, model',
          '暂未支持'
        ],
        [
          'Eval Runs',
          'GET/DELETE',
          '/v1/evals/{eval_id}/runs/{run_id}',
          'eval_id, run_id',
          '暂未支持'
        ]
      ],
      openAiText:
        'Chat Completions 与 Responses 均按 OpenAI 官方请求体转发。本节展示同步和流式文本生成。流式输出会以 text/event-stream 持续返回增量内容，适合边生成边展示。Chat Completions 的 stored completion 查询、更新、删除暂未支持。',
      requestParamsTitle: '调用参数',
      paramFieldHeaders: ['参数', '类型 / 示例', '说明'],
      textRequestParams: [
        [
          'model',
          'string，必填',
          `模型名称，例如 gpt-5.5；会按 ${siteName.value} 的模型权限、渠道选择和计费策略处理。`
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
      responseParamsTitle: '返回参数',
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
        ['usage', 'object | null', `Token 用量，${siteName.value} 会用于用量记录和结算。`]
      ],
      openAiTextAsync: `Responses 后台文本任务按官方 background 参数创建。${siteName.value} 要求 background=true 时 store 不能为 false；创建后台任务时不支持直接 stream=true，可在查询接口透传 stream=true 恢复流式结果。后台任务只支持 key-backed OpenAI 通道，不走 OpenAI OAuth/Codex 凭证通道。`,
      textAsyncRequestParams: [
        ['model', 'string，必填', 'Responses 主模型，例如 gpt-5.5。'],
        ['input', 'string | array，必填', '后台任务的输入内容。'],
        ['instructions', 'string', '系统级指令，适合放置任务要求或输出约束。'],
        ['background', 'boolean，必填 true', '设置为 true 创建后台 Response。'],
        [
          'store',
          'boolean',
          `background=true 时需要保存响应；${siteName.value} 要求 store 不能为 false。`
        ],
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
        ['usage', 'object | null', `终态返回的 Token 用量，${siteName.value} 会用于记录和结算。`]
      ],
      openAiImage:
        'Images 支持文生图、图生图/局部编辑和图片变体。生成接口使用 JSON 请求体；编辑接口支持 JSON images 数组或 multipart/form-data 上传图片，且 prompt 为必填；变体接口使用 multipart/form-data，且官方仅支持 dall-e-2。流式输出会以 text/event-stream 返回生成过程中的 partial image，适合展示预览进度。',
      imageRequestParams: [
        [
          'model',
          'string，必填',
          `图片模型，例如 gpt-image-2；会按 ${siteName.value} 模型权限和渠道能力转发。`
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
        ['usage', 'object', `上游返回的图片生成用量信息，${siteName.value} 会用于用量记录和结算。`],
        [
          'stream event',
          'text/event-stream',
          'stream=true 时返回 partial image、completed、error 等事件。'
        ]
      ],
      openAiImageAsync:
        '图片后台任务通过 Responses 的 image_generation 工具创建，而不是 Images API 自身的后台任务。NeoGate 扩展支持通过 image_format 控制异步图片结果返回 base64 或 URL。可用于文生图异步和图生图异步，创建后使用 Responses 查询、恢复流式结果或取消。',
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
        ['tools[].background', 'string', '背景模式，例如 transparent 或 opaque；取决于所选模型和上游能力。'],
        ['tools[].output_format', 'string', '图片输出格式，例如 png、jpeg 或 webp。'],
        [
          'background',
          'boolean',
          `设置为 true 创建后台 Response；${siteName.value} 的图片异步任务使用该模式。`
        ],
        [
          'store',
          'boolean',
          `background=true 时需要保存响应；${siteName.value} 要求 store 不能为 false。`
        ],
        [
          'stream',
          'boolean',
          '创建后台任务时不要设置为 true；需要流式结果时在查询接口追加 ?stream=true。'
        ],
        ['image_format', 'string', '控制异步图片结果格式，可选 base64、url 或 both；默认 base64。']
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
        ['output[].result', 'string', '完成时返回 Base64 图片内容；image_format=url 时可省略。'],
        ['output[].url', 'string', '仅在 image_format 为 url 或 both 时返回。'],
        ['error', 'object | null', '失败时包含 code 与 message；成功时通常为空。'],
        ['usage', 'object | null', `终态返回的用量信息，${siteName.value} 会用于记录和结算。`]
      ],
      openAiVideo: `Videos API 按 OpenAI 官方异步任务模型转发。创建视频后会返回 video job，可通过查询接口轮询状态；任务完成后使用 content 接口下载 MP4。${siteName.value} 当前支持创建、查询和下载内容，列表、删除、编辑、扩展和 remix 暂未支持。`,
      videoRequestParams: [
        [
          'model',
          'string',
          `视频模型，例如 sora-2 或 sora-2-pro；会按 ${siteName.value} 模型权限和渠道能力转发。`
        ],
        ['prompt', 'string，必填', '描述要生成或编辑的视频内容、镜头、动作、场景和光线。'],
        [
          'input_reference',
          'object | file',
          '可选参考图；官方 JSON 结构可传 image_url 或 file_id，multipart 调用可上传参考文件。'
        ],
        [
          'size',
          'string',
          '视频尺寸，例如 720x1280、1280x720、1024x1792 或 1792x1024；可用尺寸以模型和上游为准。'
        ],
        ['seconds', 'string | number', '视频时长，例如 4、8 或 12 秒；可用时长以模型和上游为准。'],
        ['video.id', 'string', '编辑或扩展视频时引用已完成的视频 ID。'],
        ['after / limit / order', 'string | number', '列表接口分页和排序参数；当前暂未支持。']
      ],
      videoResponseParams: [
        ['id', 'string', '视频任务 ID，例如 video_123；用于查询状态、下载内容或后续引用。'],
        ['object', '"video"', '返回对象类型。'],
        ['created_at / completed_at', 'integer | null', '任务创建和完成时间，Unix 秒级时间戳。'],
        ['status', 'string', '任务状态，例如 queued、in_progress、completed 或 failed。'],
        ['progress', 'number', '上游返回的近似完成百分比。'],
        ['model', 'string', '实际使用的视频模型。'],
        ['prompt', 'string', '创建、编辑、扩展或 remix 使用的提示词。'],
        ['size', 'string', '输出视频尺寸。'],
        ['seconds', 'string', '输出视频时长；扩展任务可能表示拼接后的总时长。'],
        ['expires_at', 'integer | null', '可下载资产过期时间，若上游返回则透传。'],
        ['error', 'object | null', '失败时包含 code 与 message；成功时通常为空。']
      ],
      openAiEmbeddings: `Embeddings 接口按 OpenAI 官方 JSON 请求体转发，适合 RAG、语义搜索、去重和召回场景。请求中的 model 会走 ${siteName.value} 的模型权限、渠道选择、计费和用量记录。`
    }
  }

  return {
    openAiTitle: '2. OpenAI-compatible APIs',
    openAiQuickStartTitle: '2.1 Quick start',
    openAiTextTitle: '2.2 Text generation',
    openAiTextAsyncTitle: '2.3 Text generation async',
    openAiImageTitle: '2.4 Images',
    openAiImageAsyncTitle: '2.5 Images async',
    openAiVideoTitle: '2.6 Videos',
    openAiEmbeddingsTitle: '2.7 Embeddings',
    openAiModelsTitle: '2.8 Models',
    openAiSdkTitle: '2.9 SDK examples',
    urlPathsTitle: 'URL paths',
    openAiTextPaths: [
      ['POST', `${openAiBaseUrl.value}/chat/completions`, 'Chat Completions'],
      ['POST', `${openAiBaseUrl.value}/responses`, 'Responses']
    ],
    openAiTextAsyncPaths: [
      ['POST', `${openAiBaseUrl.value}/responses`, 'Create background task'],
      ['GET', `${openAiBaseUrl.value}/responses/{response_id}`, 'Retrieve result'],
      ['GET', `${openAiBaseUrl.value}/responses/{response_id}?stream=true`, 'Resume stream'],
      ['POST', `${openAiBaseUrl.value}/responses/{response_id}/cancel`, 'Cancel task']
    ],
    openAiImagePaths: [
      ['POST', `${openAiBaseUrl.value}/images/generations`, 'Image generation'],
      ['POST', `${openAiBaseUrl.value}/images/edits`, 'Image edits'],
      ['POST', `${openAiBaseUrl.value}/images/variations`, 'Image variations']
    ],
    openAiImageAsyncPaths: [
      ['POST', `${openAiBaseUrl.value}/responses`, 'Create image background task via Responses'],
      ['GET', `${openAiBaseUrl.value}/responses/{response_id}`, 'Retrieve result'],
      ['GET', `${openAiBaseUrl.value}/responses/{response_id}?stream=true`, 'Resume stream'],
      ['POST', `${openAiBaseUrl.value}/responses/{response_id}/cancel`, 'Cancel task']
    ],
    openAiVideoPaths: [
      ['POST', `${openAiBaseUrl.value}/videos`, 'Create video task'],
      ['GET', `${openAiBaseUrl.value}/videos/{video_id}`, 'Retrieve video task'],
      ['GET', `${openAiBaseUrl.value}/videos/{video_id}/content`, 'Download video content']
    ],
    openAiEmbeddingsPaths: [['POST', `${openAiBaseUrl.value}/embeddings`, 'Create embeddings']],
    openAiModelsPaths: [
      ['GET', `${openAiBaseUrl.value}/models`, 'List models'],
      ['GET', `${openAiBaseUrl.value}/models/{model}`, 'Retrieve model']
    ],
    openAiTextInterfaces: [
      {
        title: 'Chat Completions',
        method: 'POST',
        path: `${openAiBaseUrl.value}/chat/completions`,
        description: 'Create synchronous or streaming text generation with messages.',
        requestParams: [
          ['model', 'string, required', 'Model name, for example gpt-5.5.'],
          ['messages', 'array, required', 'Conversation messages with role and content.'],
          ['stream', 'boolean', 'When true, returns streaming deltas.'],
          ['tools / tool_choice', 'array | object', 'Tool definitions and selection strategy.']
        ],
        responseFields: [
          ['id', 'string', 'Response ID.'],
          ['choices[]', 'array', 'Candidate results.'],
          ['choices[].message.content', 'string', 'Non-streaming text result.'],
          ['choices[].delta.content', 'string', 'Streaming text delta.'],
          ['usage', 'object | null', 'Token usage.']
        ]
      },
      {
        title: 'Responses Create',
        method: 'POST',
        path: `${openAiBaseUrl.value}/responses`,
        description: 'Create text generation with the Responses input format.',
        requestParams: [
          ['model', 'string, required', 'Model name, for example gpt-5.5.'],
          [
            'input',
            'string | array, required',
            'Input string, message array, or multimodal content.'
          ],
          ['instructions', 'string', 'System-level instructions.'],
          ['stream', 'boolean', 'When true, returns streaming events.'],
          ['text.format', 'object', 'Structured output settings.']
        ],
        responseFields: [
          ['id', 'string', 'Response ID.'],
          ['status', 'string', 'Response status, such as completed or failed.'],
          ['output[]', 'array', 'Responses output items.'],
          ['output[].content[].text', 'string', 'Text output.'],
          ['usage', 'object | null', 'Token usage.']
        ]
      }
    ],
    openAiTextAsyncInterfaces: [
      {
        title: 'Create Background Response',
        method: 'POST',
        path: `${openAiBaseUrl.value}/responses`,
        description: 'Create a background text task.',
        requestParams: [
          ['model', 'string, required', 'Responses model.'],
          ['input', 'string | array, required', 'Background task input.'],
          ['background', 'boolean, required true', 'Creates a background task.'],
          ['store', 'boolean', 'Background tasks must be stored and cannot set store=false.']
        ],
        responseFields: [
          ['id', 'string', 'Response ID used for retrieval or cancel.'],
          ['status', 'string', 'queued, in_progress, completed, failed, and related states.'],
          ['background', 'boolean', 'Whether this is a background task.']
        ]
      },
      {
        title: 'Retrieve / Stream / Cancel Response',
        method: 'GET / POST',
        path: `${openAiBaseUrl.value}/responses/{response_id}`,
        description: 'Retrieve a task, append ?stream=true to resume streaming, or call /cancel.',
        requestParams: [
          ['response_id', 'string, required', 'Response ID returned by creation.'],
          ['stream', 'boolean', 'Append stream=true when retrieving to resume streaming.']
        ],
        responseFields: [
          ['status', 'string', 'Current task status.'],
          ['output[]', 'array', 'Completed output.'],
          ['error', 'object | null', 'Failure information.'],
          ['usage', 'object | null', 'Final usage.']
        ]
      }
    ],
    openAiImageInterfaces: [
      {
        title: 'Images Generations',
        method: 'POST',
        path: `${openAiBaseUrl.value}/images/generations`,
        description: 'Generate images from a prompt.',
        requestParams: [
          ['model', 'string, required', 'Image model, such as gpt-image-2.'],
          ['prompt', 'string, required', 'Image description.'],
          ['size', 'string', 'Image size.'],
          ['stream', 'boolean', 'Whether to return streaming image events.']
        ],
        responseFields: [
          ['created', 'integer', 'Creation timestamp.'],
          ['data[]', 'array', 'Image results.'],
          ['data[].b64_json / url', 'string', 'Image content or URL.'],
          ['usage', 'object', 'Image generation usage.']
        ]
      },
      {
        title: 'Images Edits',
        method: 'POST',
        path: `${openAiBaseUrl.value}/images/edits`,
        description:
          'Edit or extend images. Multipart requests upload image or image[]; JSON requests use an images array with image_url or file_id references.',
        requestParams: [
          ['model', 'string, required', 'GPT Image model.'],
          ['image / image[]', 'file | file[]', 'Input images for multipart/form-data requests.'],
          ['images', 'array', 'Input images for JSON requests, using image_url or file_id.'],
          ['prompt', 'string, required', 'Edit prompt.'],
          [
            'mask',
            'file | object',
            'Optional edit mask; upload a file for multipart or use image_url/file_id for JSON.'
          ],
          ['size', 'string', 'Output size.']
        ],
        responseFields: [
          ['created', 'integer', 'Creation timestamp.'],
          ['data[]', 'array', 'Edited image results.'],
          ['data[].b64_json / url', 'string', 'Image content or URL.']
        ]
      },
      {
        title: 'Images Variations',
        method: 'POST',
        path: `${openAiBaseUrl.value}/images/variations`,
        description: 'Create image variations. The official endpoint only supports dall-e-2.',
        requestParams: [
          ['model', '"dall-e-2", required', 'The variations endpoint only supports dall-e-2.'],
          ['image', 'file, required', 'Input image.'],
          ['n', 'integer', 'Number of variations to generate.'],
          ['size', 'string', 'Output size.']
        ],
        responseFields: [
          ['created', 'integer', 'Creation timestamp.'],
          ['data[]', 'array', 'Variation image results.'],
          ['data[].b64_json / url', 'string', 'Image content or URL.']
        ]
      }
    ],
    openAiImageAsyncInterfaces: [
      {
        title: 'Responses Image Task',
        method: 'POST',
        path: `${openAiBaseUrl.value}/responses`,
        description: 'Create an image background task through the Responses image_generation tool. For edits, provide input_image and set action=edit.',
        requestParams: [
          ['model', 'string, required', 'Responses model.'],
          ['input', 'string | array, required', 'Text-to-image or image-to-image input.'],
          ['tools[].type', '"image_generation"', 'Enables the image generation tool.'],
          ['tools[].action', 'generate | edit | auto', 'Set edit when editing an input image.'],
          ['background', 'boolean, required true', 'Creates a background task.'],
          [
            'image_format',
            'base64 | url | both',
            'NeoGate extension controlling image result format.'
          ]
        ],
        responseFields: [
          ['id', 'string', 'Response ID.'],
          ['status', 'string', 'Background task status.'],
          ['output[].result', 'string', 'Base64 image content.'],
          ['output[].url', 'string', 'URL image result.']
        ]
      }
    ],
    openAiVideoInterfaces: [
      {
        title: 'Videos Create',
        method: 'POST',
        path: `${openAiBaseUrl.value}/videos`,
        description: 'Create a video generation task.',
        requestParams: [
          ['model', 'string, required', 'Video model, such as sora-2.'],
          ['prompt', 'string, required', 'Video description.'],
          ['input_reference', 'file | object', 'Optional reference image.'],
          ['size', 'string', 'Video size.'],
          ['seconds', 'string | number', 'Video duration.']
        ],
        responseFields: [
          ['id', 'string', 'Video task ID.'],
          ['status', 'string', 'Task status.'],
          ['progress', 'number', 'Task progress.'],
          ['error', 'object | null', 'Failure information.']
        ]
      },
      {
        title: 'Videos Retrieve / Content',
        method: 'GET',
        path: `${openAiBaseUrl.value}/videos/{video_id}`,
        description: 'Retrieve a video task; use /content to download the completed MP4.',
        requestParams: [['video_id', 'string, required', 'Video task ID.']],
        responseFields: [
          ['status', 'string', 'Task status.'],
          ['completed_at', 'integer | null', 'Completion timestamp.'],
          ['content', 'binary', '/content returns the video file.']
        ]
      }
    ],
    openAiEmbeddingsInterfaces: [
      {
        title: 'Embeddings',
        method: 'POST',
        path: `${openAiBaseUrl.value}/embeddings`,
        description: 'Create text embeddings.',
        requestParams: [
          ['model', 'string, required', 'Embedding model.'],
          ['input', 'string | array, required', 'Text to embed.'],
          ['encoding_format', 'string', 'Return format, such as float.']
        ],
        responseFields: [
          ['data[]', 'array', 'Embedding results.'],
          ['data[].embedding', 'number[]', 'Embedding vector.'],
          ['usage', 'object', 'Token usage.']
        ]
      }
    ],
    openAiModelsInterfaces: [
      {
        title: 'Models',
        method: 'GET',
        path: `${openAiBaseUrl.value}/models`,
        description: 'List models callable by the current API key.',
        requestParams: [],
        responseFields: [
          ['data[]', 'array', 'Model list.'],
          ['data[].id', 'string', 'Model ID.'],
          ['data[].owned_by', 'string', 'Model owner.']
        ]
      }
    ],
    videoWorkflowTitle: 'Video workflow',
    videoWorkflowItems: [
      [
        'Create task',
        'Call /v1/videos to create a video task with a text prompt or an input_reference image.'
      ],
      ['Check status', 'Use /v1/videos/{video_id} to check status, progress, and failure details.'],
      [
        'Download content',
        'When completed, call /v1/videos/{video_id}/content to download the MP4 file.'
      ]
    ],
    videoNotes: [
      [
        'Reference upload',
        'Video creation with a reference image uses multipart/form-data and passes the image file as input_reference.'
      ],
      [
        'Task status',
        'The status can be queued, in_progress, completed, or failed; inspect error when a task fails.'
      ]
    ],
    endpointHeaders: ['Module', 'Method', 'Official path', 'Description', 'Status'],
    openAiIntro: `OpenAI-compatible APIs use Bearer Token auth. Set the Base URL to the ${siteName.value} /v1 URL. The table follows the official OpenAI API reference; other APIs show their current support status in the table.`,
    openAiAuthItems: [
      ['Base URL', openAiBaseUrl.value],
      ['Auth header', 'Authorization: Bearer YOUR_NEOGATE_API_KEY'],
      ['Content-Type', 'application/json; image and video upload APIs use multipart/form-data']
    ],
    openAiEndpoints: [
      ['Models', 'GET', '/v1/models', '-', 'Supported'],
      ['Models', 'GET', '/v1/models/{model}', 'model', 'Supported'],
      ['Models', 'DELETE', '/v1/models/{model}', 'model', 'Not supported'],
      ['Chat Completions', 'POST', '/v1/chat/completions', 'model, messages, stream', 'Supported'],
      [
        'Chat Completions',
        'GET',
        '/v1/chat/completions/{completion_id}',
        'completion_id',
        'Not supported'
      ],
      [
        'Chat Completions',
        'GET',
        '/v1/chat/completions/{completion_id}/messages',
        'completion_id',
        'Not supported'
      ],
      [
        'Chat Completions',
        'PATCH',
        '/v1/chat/completions/{completion_id}',
        'completion_id, metadata',
        'Not supported'
      ],
      [
        'Chat Completions',
        'DELETE',
        '/v1/chat/completions/{completion_id}',
        'completion_id',
        'Not supported'
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
      ['Responses', 'DELETE', '/v1/responses/{response_id}', 'response_id', 'Not supported'],
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
        'model, image/image[] or images, prompt, mask, size, n, stream, partial_images',
        'Supported (streaming)'
      ],
      ['Images', 'POST', '/v1/images/variations', 'model=dall-e-2, image, size, n', 'Supported'],
      [
        'Videos',
        'POST',
        '/v1/videos',
        'model, prompt, input_reference, size, seconds',
        'Supported'
      ],
      ['Videos', 'GET', '/v1/videos', 'limit, after, order', 'Not supported'],
      ['Videos', 'GET', '/v1/videos/{video_id}', 'video_id', 'Supported'],
      ['Videos', 'DELETE', '/v1/videos/{video_id}', 'video_id', 'Not supported'],
      ['Videos', 'GET', '/v1/videos/{video_id}/content', 'video_id', 'Supported'],
      ['Videos', 'POST', '/v1/videos/edits', 'prompt, video.id', 'Not supported'],
      ['Videos', 'POST', '/v1/videos/extensions', 'prompt, seconds, video.id', 'Not supported'],
      ['Videos', 'POST', '/v1/videos/{video_id}/remix', 'video_id, prompt', 'Not supported'],
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
        'Not supported'
      ],
      [
        'Audio',
        'POST',
        '/v1/audio/transcriptions',
        'model, file, language, response_format',
        'Not supported'
      ],
      ['Audio', 'POST', '/v1/audio/translations', 'model, file, response_format', 'Not supported'],
      ['Moderations', 'POST', '/v1/moderations', 'model, input', 'Supported'],
      ['Files', 'POST', '/v1/files', 'file, purpose', 'Not supported'],
      ['Files', 'GET', '/v1/files', 'purpose, limit, after', 'Not supported'],
      ['Files', 'GET', '/v1/files/{file_id}', 'file_id', 'Not supported'],
      ['Files', 'DELETE', '/v1/files/{file_id}', 'file_id', 'Not supported'],
      ['Files', 'GET', '/v1/files/{file_id}/content', 'file_id', 'Not supported'],
      ['Uploads', 'POST', '/v1/uploads', 'purpose, filename, bytes, mime_type', 'Not supported'],
      ['Uploads', 'POST', '/v1/uploads/{upload_id}/parts', 'upload_id, data', 'Not supported'],
      [
        'Uploads',
        'POST',
        '/v1/uploads/{upload_id}/complete',
        'upload_id, part_ids',
        'Not supported'
      ],
      ['Uploads', 'POST', '/v1/uploads/{upload_id}/cancel', 'upload_id', 'Not supported'],
      [
        'Batches',
        'POST',
        '/v1/batches',
        'input_file_id, endpoint, completion_window',
        'Not supported'
      ],
      ['Batches', 'GET', '/v1/batches', 'limit, after', 'Not supported'],
      ['Batches', 'GET', '/v1/batches/{batch_id}', 'batch_id', 'Not supported'],
      ['Batches', 'POST', '/v1/batches/{batch_id}/cancel', 'batch_id', 'Not supported'],
      [
        'Fine-tuning',
        'POST',
        '/v1/fine_tuning/jobs',
        'model, training_file, validation_file, hyperparameters',
        'Not supported'
      ],
      ['Fine-tuning', 'GET', '/v1/fine_tuning/jobs', 'limit, after', 'Not supported'],
      [
        'Fine-tuning',
        'GET',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}',
        'fine_tuning_job_id',
        'Not supported'
      ],
      [
        'Fine-tuning',
        'POST',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}/cancel',
        'fine_tuning_job_id',
        'Not supported'
      ],
      [
        'Fine-tuning',
        'GET',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}/events',
        'fine_tuning_job_id, limit, after',
        'Not supported'
      ],
      [
        'Fine-tuning',
        'GET',
        '/v1/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints',
        'fine_tuning_job_id, limit, after',
        'Not supported'
      ],
      [
        'Vector Stores',
        'POST',
        '/v1/vector_stores',
        'name, file_ids, expires_after',
        'Not supported'
      ],
      ['Vector Stores', 'GET', '/v1/vector_stores', 'limit, after, before', 'Not supported'],
      [
        'Vector Stores',
        'GET/PATCH/DELETE',
        '/v1/vector_stores/{vector_store_id}',
        'vector_store_id',
        'Not supported'
      ],
      [
        'Vector Store Files',
        'POST/GET',
        '/v1/vector_stores/{vector_store_id}/files',
        'vector_store_id, file_id',
        'Not supported'
      ],
      [
        'Vector Store Files',
        'GET/DELETE',
        '/v1/vector_stores/{vector_store_id}/files/{file_id}',
        'vector_store_id, file_id',
        'Not supported'
      ],
      [
        'Vector Store File Batches',
        'POST/GET',
        '/v1/vector_stores/{vector_store_id}/file_batches',
        'vector_store_id, file_ids',
        'Not supported'
      ],
      [
        'Vector Store File Batches',
        'GET/POST',
        '/v1/vector_stores/{vector_store_id}/file_batches/{batch_id}',
        'vector_store_id, batch_id',
        'Not supported'
      ],
      [
        'Assistants',
        'POST/GET',
        '/v1/assistants',
        'model, instructions, tools, metadata',
        'Not supported'
      ],
      [
        'Assistants',
        'GET/PATCH/DELETE',
        '/v1/assistants/{assistant_id}',
        'assistant_id',
        'Not supported'
      ],
      ['Threads', 'POST', '/v1/threads', 'messages, metadata, tool_resources', 'Not supported'],
      ['Threads', 'GET/PATCH/DELETE', '/v1/threads/{thread_id}', 'thread_id', 'Not supported'],
      [
        'Thread Messages',
        'POST/GET',
        '/v1/threads/{thread_id}/messages',
        'thread_id, role, content',
        'Not supported'
      ],
      [
        'Thread Messages',
        'GET/PATCH/DELETE',
        '/v1/threads/{thread_id}/messages/{message_id}',
        'thread_id, message_id',
        'Not supported'
      ],
      [
        'Thread Runs',
        'POST/GET',
        '/v1/threads/{thread_id}/runs',
        'thread_id, assistant_id, model',
        'Not supported'
      ],
      [
        'Thread Runs',
        'GET/PATCH',
        '/v1/threads/{thread_id}/runs/{run_id}',
        'thread_id, run_id',
        'Not supported'
      ],
      [
        'Thread Runs',
        'POST',
        '/v1/threads/{thread_id}/runs/{run_id}/cancel',
        'thread_id, run_id',
        'Not supported'
      ],
      [
        'Thread Runs',
        'POST',
        '/v1/threads/{thread_id}/runs/{run_id}/submit_tool_outputs',
        'thread_id, run_id, tool_outputs',
        'Not supported'
      ],
      [
        'Realtime',
        'POST',
        '/v1/realtime/sessions',
        'model, voice, modalities, instructions',
        'Not supported'
      ],
      [
        'Realtime',
        'POST',
        '/v1/realtime/transcription_sessions',
        'input_audio_format, input_audio_transcription',
        'Not supported'
      ],
      [
        'Evals',
        'POST/GET',
        '/v1/evals',
        'name, data_source_config, testing_criteria',
        'Not supported'
      ],
      ['Evals', 'GET/PATCH/DELETE', '/v1/evals/{eval_id}', 'eval_id', 'Not supported'],
      [
        'Eval Runs',
        'POST/GET',
        '/v1/evals/{eval_id}/runs',
        'eval_id, data_source, model',
        'Not supported'
      ],
      [
        'Eval Runs',
        'GET/DELETE',
        '/v1/evals/{eval_id}/runs/{run_id}',
        'eval_id, run_id',
        'Not supported'
      ]
    ],
    openAiText:
      'Chat Completions and Responses are forwarded with the official OpenAI request body. This section shows synchronous and streaming text generation. Streaming returns incremental content over text/event-stream, which is useful when the UI should render while the model is still generating. Stored Chat Completion retrieve, update, message listing, and delete are not currently supported.',
    requestParamsTitle: 'Request parameters',
    paramFieldHeaders: ['Parameter', 'Type / example', 'Description'],
    textRequestParams: [
      [
        'model',
        'string, required',
        `Model name, for example gpt-5.5. ${siteName.value} applies model permissions, routing, and billing policy.`
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
    responseParamsTitle: 'Response parameters',
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
      [
        'usage',
        'object | null',
        `Token usage used by ${siteName.value} for records and settlement.`
      ]
    ],
    openAiTextAsync: `Background text Responses follow the official background parameter. ${siteName.value} requires store not to be false when background=true. Create-time streaming is not supported for background tasks; retrieve can pass through stream=true to resume streamed results. Background tasks require key-backed OpenAI channels and do not use OpenAI OAuth/Codex credential channels.`,
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
        `Required for background responses; ${siteName.value} does not allow store=false.`
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
      [
        'usage',
        'object | null',
        `Final token usage used by ${siteName.value} for records and settlement.`
      ]
    ],
    openAiImage:
      'Images supports text-to-image, image edits, and image variations. Generations use a JSON body; edits require a prompt and support either a JSON images array or multipart/form-data image uploads. Variations use multipart/form-data and the official endpoint only supports dall-e-2. Streaming returns partial images over text/event-stream, which is useful for showing generation progress.',
    imageRequestParams: [
      [
        'model',
        'string, required',
        `Image model, for example gpt-image-2. ${siteName.value} still applies model permissions and upstream routing.`
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
        `Image generation usage returned by the upstream and used for ${siteName.value} billing records.`
      ],
      [
        'stream event',
        'text/event-stream',
        'With stream=true, events include partial image, completed, and error states.'
      ]
    ],
    openAiImageAsync:
      'Background image tasks are created through the Responses image_generation tool, not through a background mode on the Images API itself. NeoGate extends the request with image_format so async image results can return base64 or a URL. Use it for async text-to-image and image-to-image, then retrieve, resume streaming, or cancel through Responses.',
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
      ['tools[].background', 'string', 'Background mode, such as transparent or opaque, subject to model and upstream support.'],
      ['tools[].output_format', 'string', 'Output image format, such as png, jpeg, or webp.'],
      [
        'image_format',
        'string',
        'Controls the async image result format. Use base64, url, or both; the default is base64.'
      ],
      [
        'background',
        'boolean',
        `Set true to create a background Response; ${siteName.value} async image tasks use this mode.`
      ],
      [
        'store',
        'boolean',
        `Required for background responses; ${siteName.value} does not allow store=false.`
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
      [
        'output[].result',
        'string',
        'Base64 image content returned when generation completes; omitted when image_format=url.'
      ],
      ['output[].url', 'string', 'Returned when image_format is url or both.'],
      ['error', 'object | null', 'On failure, includes code and message; usually null on success.'],
      [
        'usage',
        'object | null',
        `Final usage data used by ${siteName.value} for records and settlement.`
      ]
    ],
    openAiVideo: `The Videos API is forwarded using OpenAI's official async job model. Create returns a video job, retrieve polls status, and content downloads the completed MP4. ${siteName.value} currently supports create, retrieve, and content download; list, delete, edits, extensions, and remix are not currently supported.`,
    videoRequestParams: [
      [
        'model',
        'string',
        `Video model, such as sora-2 or sora-2-pro. ${siteName.value} still applies model permissions and upstream routing.`
      ],
      [
        'prompt',
        'string, required',
        'Describes the video content, shot, motion, setting, and lighting.'
      ],
      [
        'input_reference',
        'object | file',
        'Optional reference image. Official JSON accepts image_url or file_id; multipart requests can upload a reference file.'
      ],
      [
        'size',
        'string',
        'Video size, such as 720x1280, 1280x720, 1024x1792, or 1792x1024. Availability depends on the model and upstream.'
      ],
      [
        'seconds',
        'string | number',
        'Clip duration, such as 4, 8, or 12 seconds. Availability depends on the model and upstream.'
      ],
      ['video.id', 'string', 'Completed video ID used by edit or extension requests.'],
      [
        'after / limit / order',
        'string | number',
        'Pagination and sort parameters for list videos; not currently supported.'
      ]
    ],
    videoResponseParams: [
      [
        'id',
        'string',
        'Video job ID, such as video_123, used to retrieve status, download content, or reference later.'
      ],
      ['object', '"video"', 'Object type returned by the Videos API.'],
      [
        'created_at / completed_at',
        'integer | null',
        'Unix timestamps for when the job was created and completed.'
      ],
      ['status', 'string', 'Task status, such as queued, in_progress, completed, or failed.'],
      ['progress', 'number', 'Approximate completion percentage returned by the upstream.'],
      ['model', 'string', 'Video model that produced the job.'],
      ['prompt', 'string', 'Prompt used for creation, editing, extension, or remix.'],
      ['size', 'string', 'Output video size.'],
      ['seconds', 'string', 'Output duration; extensions may return the stitched total duration.'],
      [
        'expires_at',
        'integer | null',
        'Downloadable asset expiration time when returned by the upstream.'
      ],
      ['error', 'object | null', 'On failure, includes code and message; usually null on success.']
    ],
    openAiEmbeddings: `Embeddings are forwarded with the official OpenAI JSON request body and are useful for RAG, semantic search, deduplication, and retrieval. The requested model still uses ${siteName.value} model permissions, routing, billing, and usage records.`
  }
})
</script>

<template>
  <section id="openai" class="docs-section">
    <div class="docs-section-heading">
      <h2>{{ content.openAiTitle }}</h2>
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
            v-for="[name, method, path, , status] in content.openAiEndpoints"
            :key="`${name}-${method}-${path}`"
          >
            <td>{{ name }}</td>
            <td>
              <span class="interface-method">{{ method }}</span>
            </td>
            <td>
              <code>{{ path }}</code>
            </td>
            <td>{{ endpointDescription(name, method, path) }}</td>
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
          <h2>{{ content.openAiQuickStartTitle }}</h2>
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
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiQuickStart }}</code></pre>
          </div>
        </article>
      </section>

      <section id="openai-text" class="docs-subsection">
        <div class="docs-section-heading docs-subsection-heading">
          <h2>{{ content.openAiTextTitle }}</h2>
          <p>{{ content.openAiText }}</p>
        </div>
        <InterfaceEndpointList
          :items="content.openAiTextInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
        <article class="docs-step-card">
          <h3>Responses Create</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiResponses)"
            />
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiResponses }}</code></pre>
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
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiStream }}</code></pre>
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
          <h2>{{ content.openAiTextAsyncTitle }}</h2>
          <p>{{ content.openAiTextAsync }}</p>
        </div>
        <InterfaceEndpointList
          :items="content.openAiTextAsyncInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
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
          <h2>{{ content.openAiImageTitle }}</h2>
          <p>{{ content.openAiImage }}</p>
        </div>
        <InterfaceEndpointList
          :items="content.openAiImageInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
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
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiImageEdit }}</code></pre>
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
          <h2>{{ content.openAiImageAsyncTitle }}</h2>
          <p>{{ content.openAiImageAsync }}</p>
        </div>
        <InterfaceEndpointList
          :items="content.openAiImageAsyncInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
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

      <section id="openai-videos" class="docs-subsection">
        <div class="docs-section-heading docs-subsection-heading">
          <h2>{{ content.openAiVideoTitle }}</h2>
          <p>{{ content.openAiVideo }}</p>
        </div>
        <InterfaceEndpointList
          :items="content.openAiVideoInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
        <article class="docs-step-card">
          <h3>{{ content.videoWorkflowTitle }}</h3>
          <div class="docs-check-list docs-inner-check-list">
            <article
              v-for="[title, text] in content.videoWorkflowItems"
              :key="title"
              class="docs-check-item"
            >
              <h3>{{ title }}</h3>
              <p>{{ text }}</p>
            </article>
          </div>
        </article>
        <div class="docs-check-list">
          <article v-for="[title, text] in content.videoNotes" :key="title" class="docs-check-item">
            <h3>{{ title }}</h3>
            <p>{{ text }}</p>
          </article>
        </div>
        <article class="docs-step-card">
          <h3>Create Video</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiVideoCreate)"
            />
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiVideoCreate }}</code></pre>
          </div>
        </article>
        <article class="docs-step-card">
          <h3>Create Video With Reference</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiVideoCreateWithReference)"
            />
            <pre
              class="docs-code-sample docs-inner-code"
            ><code>{{ openAiVideoCreateWithReference }}</code></pre>
          </div>
        </article>
        <article class="docs-step-card">
          <h3>Retrieve Video</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiVideoRetrieve)"
            />
            <pre
              class="docs-code-sample docs-inner-code"
            ><code>{{ openAiVideoRetrieve }}</code></pre>
          </div>
        </article>
        <article class="docs-step-card">
          <h3>Download Video Content</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiVideoContent)"
            />
            <pre
              class="docs-code-sample docs-inner-code"
            ><code>{{ openAiVideoContent }}</code></pre>
          </div>
        </article>
      </section>

      <section id="openai-embeddings" class="docs-subsection">
        <div class="docs-section-heading docs-subsection-heading">
          <h2>{{ content.openAiEmbeddingsTitle }}</h2>
          <p>{{ content.openAiEmbeddings }}</p>
        </div>
        <InterfaceEndpointList
          :items="content.openAiEmbeddingsInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
        <article class="docs-step-card">
          <h3>Embeddings</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiEmbeddings)"
            />
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiEmbeddings }}</code></pre>
          </div>
        </article>
      </section>

      <section id="openai-models" class="docs-subsection">
        <div class="docs-section-heading docs-subsection-heading">
          <h2>{{ content.openAiModelsTitle }}</h2>
        </div>
        <InterfaceEndpointList
          :items="content.openAiModelsInterfaces"
          :field-headers="content.paramFieldHeaders"
          :request-title="content.requestParamsTitle"
          :response-title="content.responseParamsTitle"
        />
        <article class="docs-step-card">
          <h3>Models</h3>
          <div class="docs-copy-block">
            <el-button
              :icon="DocumentCopy"
              text
              :aria-label="t('copy')"
              @click="copyDocText(openAiModels)"
            />
            <pre class="docs-code-sample docs-inner-code"><code>{{ openAiModels }}</code></pre>
          </div>
        </article>
      </section>

      <section id="openai-sdk" class="docs-subsection">
        <div class="docs-section-heading docs-subsection-heading">
          <h2>{{ content.openAiSdkTitle }}</h2>
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
                  <pre class="docs-code-sample docs-inner-code"><code>{{ openAiNode }}</code></pre>
                </div>
              </div>
            </el-tab-pane>
          </el-tabs>
        </article>
      </section>
    </div>
  </section>
</template>
