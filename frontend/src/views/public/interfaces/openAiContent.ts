import { computed } from 'vue'
import { useLocale } from '../../../composables/useLocale'
import { useSiteBrand } from '../../../composables/useSiteBrand'
import { openAiEndpointRows, toEndpointDisplayRows } from './endpointRows'

export interface EndpointSample {
  title: string
  code: string
}

/**
 * Content for the OpenAI-compatible API docs section.
 * curl samples rely on BASE_URL / API_KEY exported in the quick-start example.
 */
export function useOpenAiContent() {
  const { locale } = useLocale()
  const { siteName } = useSiteBrand()
  const siteOrigin = computed(() => window.location.origin)

  const openAiBaseUrl = computed(() => `${siteOrigin.value}/v1`)

  const openAiEndpoints = computed(() =>
    toEndpointDisplayRows(
      openAiEndpointRows,
      locale.value === 'zh-CN' ? 'zh' : 'en',
      siteName.value
    )
  )

  const quickStart = computed(() => {
    const apiKeyPlaceholder = locale.value === 'zh-CN' ? '<你的 API Key>' : '<your API key>'
    return `export BASE_URL="${siteOrigin.value}"
export API_KEY="${apiKeyPlaceholder}"

curl "$BASE_URL/v1/chat/completions" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [
      { "role": "user", "content": "用一句话介绍 ${siteName.value}" }
    ]
  }'`
  })

  const chatCompletionsStream = `curl "$BASE_URL/v1/chat/completions" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [
      { "role": "user", "content": "连续输出 3 个要点" }
    ],
    "stream": true
  }'`

  const responsesCreate = `curl "$BASE_URL/v1/responses" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "写一个 TypeScript 防抖函数",
    "stream": false
  }'`

  const responsesStream = `curl -N "$BASE_URL/v1/responses" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "连续输出 3 个排查 API 问题的步骤",
    "stream": true
  }'`

  const responseBackground = `curl "$BASE_URL/v1/responses" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "input": "生成一份 500 字的接口迁移说明",
    "background": true,
    "store": true
  }'`

  const responseStreamRetrieve = `curl -N "$BASE_URL/v1/responses/resp_123?stream=true" \\
  -H "Authorization: Bearer $API_KEY"`

  const responseImageGeneration = `curl "$BASE_URL/v1/responses" \\
  -H "Authorization: Bearer $API_KEY" \\
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

  const responseImageEdit = `IMG_B64="$(base64 < input.jpg | tr -d '\\n')"

curl "$BASE_URL/v1/responses" \\
  -H "Authorization: Bearer $API_KEY" \\
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

  const imageGeneration = `curl "$BASE_URL/v1/images/generations" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-image-2",
    "prompt": "A compact glass teapot on a walnut table",
    "size": "1024x1024"
  }'`

  const imageGenerationStream = `curl -N "$BASE_URL/v1/images/generations" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-image-2",
    "prompt": "A compact glass teapot on a walnut table",
    "size": "1024x1024",
    "stream": true,
    "partial_images": 2
  }'`

  const imageEdit = `curl "$BASE_URL/v1/images/edits" \\
  -H "Authorization: Bearer $API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image[]=@input.png" \\
  -F "prompt=Add a soft morning light through the window" \\
  -F "size=1024x1024"`

  const imageEditStream = `curl -N "$BASE_URL/v1/images/edits" \\
  -H "Authorization: Bearer $API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image[]=@input.png" \\
  -F "prompt=Add a soft morning light through the window" \\
  -F "size=1024x1024" \\
  -F "stream=true" \\
  -F "partial_images=2"`

  const imageVariation = `curl "$BASE_URL/v1/images/variations" \\
  -H "Authorization: Bearer $API_KEY" \\
  -F "model=dall-e-2" \\
  -F "image=@input.png" \\
  -F "size=1024x1024"`

  const videoCreate = `curl "$BASE_URL/v1/videos" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "sora-2",
    "prompt": "Wide tracking shot of a teal coupe driving through a desert highway",
    "size": "1280x720",
    "seconds": "8"
  }'`

  const videoCreateWithReference = `curl "$BASE_URL/v1/videos" \\
  -H "Authorization: Bearer $API_KEY" \\
  -F "model=sora-2" \\
  -F "prompt=A lantern-lit street slowly filling with rain reflections" \\
  -F "size=720x1280" \\
  -F "seconds=4" \\
  -F "input_reference=@reference.png;type=image/png"`

  const assetCreate = `curl "$BASE_URL/v1/assets" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "sd_2.0_discount",
    "type": "image",
    "url": "https://example.com/reference-1.png",
    "name": "reference-1"
  }'`

  const assetRetrieve = `curl "$BASE_URL/v1/assets/asset_123" \\
  -H "Authorization: Bearer $API_KEY"`

  const videoCreateWithAssets = `curl "$BASE_URL/v1/videos" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "sd_2.0_discount",
    "prompt": "Combine both references into a cinematic tracking shot",
    "seconds": 5,
    "ratio": "16:9",
    "resolution": "720p",
    "content": [
      {
        "type": "image_url",
        "role": "reference_image",
        "image_url": { "url": "asset://asset_reference_1" }
      },
      {
        "type": "image_url",
        "role": "reference_image",
        "image_url": { "url": "asset://asset_reference_2" }
      }
    ]
  }'`

  const videoRetrieve = `curl "$BASE_URL/v1/videos/video_123" \\
  -H "Authorization: Bearer $API_KEY"`

  const videoContent = `curl "$BASE_URL/v1/videos/video_123/content" \\
  -H "Authorization: Bearer $API_KEY" \\
  --output video.mp4`

  const audioTranscription = `curl "$BASE_URL/v1/audio/transcriptions" \\
  -H "Authorization: Bearer $API_KEY" \\
  -F "file=@meeting.mp3" \\
  -F "model=fun-asr-flash-2026-06-15" \\
  -F "response_format=json"`

  const embeddingsSample = computed(
    () => `curl "$BASE_URL/v1/embeddings" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "text-embedding-3-small",
    "input": [
      "${siteName.value} routes OpenAI-compatible API requests.",
      "Embeddings can be used for search and retrieval."
    ],
    "encoding_format": "float"
  }'`
  )

  const modelsSample = `curl "$BASE_URL/v1/models" \\
  -H "Authorization: Bearer $API_KEY"`

  const responseRetrieve = `curl "$BASE_URL/v1/responses/resp_123" \\
  -H "Authorization: Bearer $API_KEY"`

  const responseCancel = `curl "$BASE_URL/v1/responses/resp_123/cancel" \\
  -X POST \\
  -H "Authorization: Bearer $API_KEY"`

  const pythonInstall = 'pip install openai'

  const python = computed(
    () => `from openai import OpenAI

client = OpenAI(
    api_key="YOUR_API_KEY",
    base_url="${openAiBaseUrl.value}",
)

response = client.chat.completions.create(
    model="gpt-5.5",
    messages=[
        {"role": "user", "content": "用一句话介绍 ${siteName.value}"}
    ],
)

print(response.choices[0].message.content)`
  )

  const nodeInstall = 'npm install openai'

  const node = computed(
    () => `import OpenAI from "openai";

const client = new OpenAI({
  apiKey: "YOUR_API_KEY",
  baseURL: "${openAiBaseUrl.value}",
});

const response = await client.responses.create({
  model: "gpt-5.5",
  input: "写一个 TypeScript 防抖函数",
});

console.log(response.output_text);`
  )

  const chatCompletionsSamples: EndpointSample[] = [
    { title: 'Chat Completions Stream', code: chatCompletionsStream }
  ]
  const responsesCreateSamples: EndpointSample[] = [
    { title: 'Responses Create', code: responsesCreate },
    { title: 'Responses Stream', code: responsesStream }
  ]
  const backgroundResponseSamples: EndpointSample[] = [
    { title: 'Create Background Response', code: responseBackground }
  ]
  const responseManageSamples: EndpointSample[] = [
    { title: 'Retrieve Response', code: responseRetrieve },
    { title: 'Retrieve Stream', code: responseStreamRetrieve },
    { title: 'Cancel Response', code: responseCancel }
  ]
  const imageGenerationsSamples: EndpointSample[] = [
    { title: 'Generations', code: imageGeneration },
    { title: 'Generations Stream', code: imageGenerationStream }
  ]
  const imageEditsSamples: EndpointSample[] = [
    { title: 'Edits', code: imageEdit },
    { title: 'Edits Stream', code: imageEditStream }
  ]
  const imageVariationsSamples: EndpointSample[] = [{ title: 'Variations', code: imageVariation }]
  const imageAsyncSamples: EndpointSample[] = [
    { title: 'Background Text to Image', code: responseImageGeneration },
    { title: 'Background Image to Image', code: responseImageEdit },
    { title: 'Retrieve Response', code: responseRetrieve },
    { title: 'Retrieve Stream', code: responseStreamRetrieve },
    { title: 'Cancel Response', code: responseCancel }
  ]
  const videoCreateSamples = computed<EndpointSample[]>(() => [
    { title: 'Create Video', code: videoCreate },
    { title: 'Create Video With Reference', code: videoCreateWithReference },
    {
      title: `Create Video With Two Assets (${siteName.value} Extension)`,
      code: videoCreateWithAssets
    }
  ])
  const videoRetrieveSamples: EndpointSample[] = [
    { title: 'Retrieve Video', code: videoRetrieve },
    { title: 'Download Video Content', code: videoContent }
  ]
  const assetCreateSamples = computed<EndpointSample[]>(() => [
    { title: `Create Asset (${siteName.value} Extension)`, code: assetCreate }
  ])
  const assetRetrieveSamples = computed<EndpointSample[]>(() => [
    { title: `Retrieve Asset (${siteName.value} Extension)`, code: assetRetrieve }
  ])
  const audioSamples: EndpointSample[] = [
    { title: 'Audio Transcription', code: audioTranscription }
  ]
  const embeddingsSamples = computed<EndpointSample[]>(() => [
    { title: 'Embeddings', code: embeddingsSample.value }
  ])
  const modelsSamples: EndpointSample[] = [{ title: 'Models', code: modelsSample }]

  const content = computed(() => {
    if (locale.value === 'zh-CN') {
      return {
        openAiTitle: '2. OpenAI 兼容接口',
        openAiQuickStartTitle: '2.1 快速开始',
        openAiTextTitle: '2.2 文本生成',
        openAiTextAsyncTitle: '2.3 文本生成（异步）',
        openAiImageTitle: '2.4 图片生成',
        openAiImageAsyncTitle: '2.5 图片生成（异步）',
        openAiVideoTitle: '2.6 视频与素材',
        openAiAudioTitle: '2.7 音频转写',
        openAiEmbeddingsTitle: '2.8 向量嵌入',
        openAiModelsTitle: '2.9 模型列表',
        openAiSdkTitle: '2.10 SDK 示例',
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
        openAiAssetPaths: [
          ['POST', openAiBaseUrl.value + '/assets', `创建素材（${siteName.value} 扩展）`],
          [
            'GET',
            openAiBaseUrl.value + '/assets/{asset_id}',
            `查询素材状态（${siteName.value} 扩展）`
          ]
        ],
        openAiAudioPaths: [
          ['POST', `${openAiBaseUrl.value}/audio/transcriptions`, '将音频转写为文本']
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
            ],
            samples: chatCompletionsSamples
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
            ],
            samples: responsesCreateSamples
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
            ],
            samples: backgroundResponseSamples
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
            ],
            samples: responseManageSamples
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
            ],
            samples: imageGenerationsSamples
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
            ],
            samples: imageEditsSamples
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
            ],
            samples: imageVariationsSamples
          }
        ],
        openAiImageAsyncInterfaces: [
          {
            title: 'Responses Image Task',
            method: 'POST',
            path: `${openAiBaseUrl.value}/responses`,
            description:
              '通过 Responses 的 image_generation 工具创建图片后台任务；编辑时传入 input_image 并设置 action=edit。',
            requestParams: [
              ['model', 'string，必填', 'Responses 主模型。'],
              ['input', 'string | array，必填', '文生图或图生图输入。'],
              ['tools[].type', '"image_generation"', '启用图片生成工具。'],
              ['tools[].action', 'generate | edit | auto', '编辑输入图片时设置为 edit。'],
              ['background', 'boolean，必填 true', '创建后台任务。'],
              ['image_format', 'base64 | url | both', `${siteName.value} 扩展，控制图片结果格式。`]
            ],
            responseFields: [
              ['id', 'string', 'Response ID。'],
              ['status', 'string', '后台任务状态。'],
              ['output[].result', 'string', 'Base64 图片内容。'],
              ['output[].url', 'string', 'URL 图片结果。']
            ],
            samples: imageAsyncSamples
          }
        ],
        openAiVideoInterfaces: [
          {
            title: 'Videos Create',
            method: 'POST',
            path: `${openAiBaseUrl.value}/videos`,
            description: `创建视频生成任务；支持 OpenAI 单参考图输入及 ${siteName.value} 多素材扩展。`,
            requestParams: [
              ['model', 'string，必填', '视频模型，例如 sora-2。'],
              ['prompt', 'string，必填', '视频内容描述。'],
              ['input_reference', 'file | object', '可选参考图。'],
              [
                'content[]',
                'array',
                `${siteName.value} 扩展。传入一个或多个图片、视频或音频参考素材。`
              ],
              ['size', 'string', '视频尺寸。'],
              [
                'seconds / duration',
                'string | number',
                `视频时长；duration 为 ${siteName.value} 扩展别名。`
              ],
              [
                'ratio / resolution',
                'string',
                `${siteName.value} 扩展。宽高比和输出清晰度；可用值以模型能力为准。`
              ],
              [
                'generate_audio',
                'boolean',
                `${siteName.value} 扩展。请求生成随视频音频；是否支持取决于模型。`
              ]
            ],
            responseFields: [
              ['id', 'string', '视频任务 ID。'],
              ['status', 'string', '任务状态。'],
              ['progress', 'number', '任务进度。'],
              ['error', 'object | null', '失败信息。']
            ],
            samples: videoCreateSamples.value
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
            ],
            samples: videoRetrieveSamples
          }
        ],
        openAiAssetInterfaces: [
          {
            title: 'Assets Create',
            method: 'POST',
            path: openAiBaseUrl.value + '/assets',
            description:
              '通过公网 URL 创建可复用的图片、视频或音频素材。素材按项目共享，不直接暴露上游素材 ID。',
            requestParams: [
              ['model', 'string，必填', '具备素材能力的视频模型。'],
              ['type', 'image | video | audio，必填', '素材类型，必须与后续视频引用字段匹配。'],
              [
                'url',
                'string，必填',
                '公网 HTTP/HTTPS URL；不支持 base64、data URL、multipart 或本地地址。'
              ],
              ['name', 'string', '可选名称，最多 50 个 Unicode 字符。']
            ],
            responseFields: [
              ['id', 'string', `${siteName.value} 素材 ID，格式为 asset_*。`],
              ['type', 'string', '素材类型。'],
              ['url', 'string', '创建时提交的公网 URL。'],
              ['name', 'string | null', '素材名称。'],
              ['status', 'string', 'processing、active、failed、expired 或 deleted。'],
              ['error', 'string', '仅在失败时返回。']
            ],
            samples: assetCreateSamples.value
          },
          {
            title: 'Assets Retrieve',
            method: 'GET',
            path: openAiBaseUrl.value + '/assets/{asset_id}',
            description: '查询素材并刷新处理状态；素材访问按当前项目隔离。',
            requestParams: [['asset_id', 'string，必填', '创建素材返回的 asset_* ID。']],
            responseFields: [
              ['id', 'string', `${siteName.value} 素材 ID。`],
              ['status', 'string', 'processing、active、failed、expired 或 deleted。'],
              ['error', 'string', '仅在失败时返回。']
            ],
            samples: assetRetrieveSamples.value
          }
        ],
        openAiAudioInterfaces: [
          {
            title: 'Audio Transcriptions',
            method: 'POST',
            path: `${openAiBaseUrl.value}/audio/transcriptions`,
            description: '上传音频并返回转写文本。',
            requestParams: [
              ['file', 'file，必填', '待转写的音频文件。'],
              ['model', 'string，必填', '音频转写模型。'],
              ['language / languages[]', 'string | array', '语言提示；不可与另一项同时使用。'],
              [
                'prompt / keywords[]',
                'string | array',
                'Fun-ASR-Flash 上下文提示，总长度不超过 400 字符。'
              ],
              [
                'response_format',
                'json | text | verbose_json | srt | vtt',
                '响应格式，默认 json。'
              ],
              [
                'timestamp_granularities[]',
                'word | segment',
                'verbose_json 的词级或句段级时间戳。'
              ],
              ['stream', 'boolean', 'Fun-ASR-Flash 返回 transcript.text.delta/done SSE。'],
              ['temperature', 'number', '仅支持 Fun-ASR-Flash 的 0。']
            ],
            responseFields: [
              ['text', 'string', '识别得到的文本内容。'],
              ['segments / words', 'array', 'verbose_json 中的句段和词级时间戳。'],
              ['usage', 'object', '按音频秒数计费的用量。']
            ],
            samples: audioSamples
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
            ],
            samples: embeddingsSamples.value
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
            ],
            samples: modelsSamples
          }
        ],
        videoWorkflowTitle: '视频生成流程',
        videoWorkflowItems: [
          [
            '准备素材（可选）',
            '调用 /v1/assets 创建素材，并轮询到 active；素材创建不产生视频生成费用。'
          ],
          [
            '创建任务',
            `调用 /v1/videos 创建任务。单参考图可使用 input_reference；多参考素材使用 ${siteName.value} 扩展 content[]。`
          ],
          ['查询状态', '通过 /v1/videos/{video_id} 查询任务状态、进度和失败原因。'],
          ['下载内容', '任务完成后调用 /v1/videos/{video_id}/content 下载 MP4 文件。']
        ],
        videoNotes: [
          [
            '参考图上传',
            `OpenAI 兼容模式使用 multipart/form-data 的 input_reference 上传一张图片；${siteName.value} 扩展可在 JSON content[] 中引用公网 URL 或 asset://asset_*。`
          ],
          [
            '多参考素材',
            'content[] 支持图片、视频和音频类型；可用数量、角色组合和媒体能力以所选模型及渠道为准。'
          ],
          [
            '素材约束',
            'asset:// 引用必须属于当前项目、类型匹配且状态为 active；同一请求中的素材必须绑定到同一可用上游。'
          ],
          [
            '任务状态',
            '返回 status 为 queued、in_progress、completed 或 failed；失败时查看 error 字段。'
          ]
        ],
        endpointHeaders: ['模块', '方法', '接口路径', '接口说明', '状态'],
        openAiIntro: `接口统一使用 Bearer Token 认证。Base URL 填写 ${siteName.value} 的 /v1 地址。下表包含 OpenAI 兼容接口和 ${siteName.value} 扩展接口；扩展能力会在状态列或说明中明确标注。`,
        openAiAuthItems: [
          ['Base URL', openAiBaseUrl.value],
          ['认证头', 'Authorization: Bearer YOUR_API_KEY'],
          ['Content-Type', 'application/json；图片、视频和音频上传接口使用 multipart/form-data']
        ],
        endpointSearchPlaceholder: '筛选接口…',
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
          [
            'object',
            'string',
            '对象类型，例如 chat.completion、chat.completion.chunk 或 response。'
          ],
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
          [
            'tools / tool_choice',
            'array | object',
            '工具定义和选择策略；后台任务会按官方字段透传。'
          ],
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
          [
            'usage',
            'object',
            `上游返回的图片生成用量信息，${siteName.value} 会用于用量记录和结算。`
          ],
          [
            'stream event',
            'text/event-stream',
            'stream=true 时返回 partial image、completed、error 等事件。'
          ]
        ],
        openAiImageAsync: `图片后台任务通过 Responses 的 image_generation 工具创建，而不是 Images API 自身的后台任务。${siteName.value} 扩展支持通过 image_format 控制异步图片结果返回 base64 或 URL。可用于文生图异步和图生图异步，创建后使用 Responses 查询、恢复流式结果或取消。`,
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
          [
            'tools[].background',
            'string',
            '背景模式，例如 transparent 或 opaque；取决于所选模型和上游能力。'
          ],
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
          [
            'image_format',
            'string',
            '控制异步图片结果格式，可选 base64、url 或 both；默认 base64。'
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
          ['output[].result', 'string', '完成时返回 Base64 图片内容；image_format=url 时可省略。'],
          ['output[].url', 'string', '仅在 image_format 为 url 或 both 时返回。'],
          ['error', 'object | null', '失败时包含 code 与 message；成功时通常为空。'],
          ['usage', 'object | null', `终态返回的用量信息，${siteName.value} 会用于记录和结算。`]
        ],
        openAiVideo: `Videos API 按 OpenAI 异步任务模型转发，${siteName.value} 另提供通用素材 API 和多参考素材扩展。创建视频后通过查询接口轮询状态，完成后使用 content 接口下载视频。当前支持创建、查询和下载；列表、删除、编辑、扩展和 remix 暂未支持。`,
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
            'OpenAI 兼容的单参考图。JSON 传 image_url，或用 multipart 上传一张图片；当前不支持 file_id。'
          ],
          [
            'content[]',
            'array',
            `${siteName.value} 扩展。使用 image_url、video_url 或 audio_url 传多个公网 URL 或 asset://asset_* 引用，并通过 role 指定用途。`
          ],
          [
            'size',
            'string',
            '视频尺寸，例如 720x1280、1280x720、1024x1792 或 1792x1024；可用尺寸以模型和上游为准。'
          ],
          [
            'seconds',
            'string | number',
            '视频时长，例如 4、8 或 12 秒；可用时长以模型和上游为准。'
          ],
          [
            'duration',
            'integer',
            `${siteName.value} 扩展。seconds 的别名；可用范围以模型和渠道为准。`
          ],
          ['ratio', 'string', `${siteName.value} 扩展。输出宽高比，例如 16:9、9:16 或 1:1。`],
          [
            'resolution',
            'string',
            `${siteName.value} 扩展。输出清晰度，例如 480p、720p 或 1080p。`
          ],
          ['generate_audio', 'boolean', `${siteName.value} 扩展。为支持的模型请求同步生成音频。`],
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
        openAiAudio:
          '音频转写接口使用 multipart/form-data 上传音频，并返回 OpenAI 兼容的转写结果。',
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
      openAiVideoTitle: '2.6 Videos and assets',
      openAiAudioTitle: '2.7 Audio transcription',
      openAiEmbeddingsTitle: '2.8 Embeddings',
      openAiModelsTitle: '2.9 Models',
      openAiSdkTitle: '2.10 SDK examples',
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
      openAiAssetPaths: [
        ['POST', openAiBaseUrl.value + '/assets', `Create asset (${siteName.value} extension)`],
        [
          'GET',
          openAiBaseUrl.value + '/assets/{asset_id}',
          `Retrieve asset status (${siteName.value} extension)`
        ]
      ],
      openAiAudioPaths: [
        ['POST', `${openAiBaseUrl.value}/audio/transcriptions`, 'Transcribe audio to text']
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
          ],
          samples: chatCompletionsSamples
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
          ],
          samples: responsesCreateSamples
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
          ],
          samples: backgroundResponseSamples
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
          ],
          samples: responseManageSamples
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
          ],
          samples: imageGenerationsSamples
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
          ],
          samples: imageEditsSamples
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
          ],
          samples: imageVariationsSamples
        }
      ],
      openAiImageAsyncInterfaces: [
        {
          title: 'Responses Image Task',
          method: 'POST',
          path: `${openAiBaseUrl.value}/responses`,
          description:
            'Create an image background task through the Responses image_generation tool. For edits, provide input_image and set action=edit.',
          requestParams: [
            ['model', 'string, required', 'Responses model.'],
            ['input', 'string | array, required', 'Text-to-image or image-to-image input.'],
            ['tools[].type', '"image_generation"', 'Enables the image generation tool.'],
            ['tools[].action', 'generate | edit | auto', 'Set edit when editing an input image.'],
            ['background', 'boolean, required true', 'Creates a background task.'],
            [
              'image_format',
              'base64 | url | both',
              `${siteName.value} extension controlling image result format.`
            ]
          ],
          responseFields: [
            ['id', 'string', 'Response ID.'],
            ['status', 'string', 'Background task status.'],
            ['output[].result', 'string', 'Base64 image content.'],
            ['output[].url', 'string', 'URL image result.']
          ],
          samples: imageAsyncSamples
        }
      ],
      openAiVideoInterfaces: [
        {
          title: 'Videos Create',
          method: 'POST',
          path: `${openAiBaseUrl.value}/videos`,
          description: `Create a video generation task with an OpenAI-compatible single image or ${siteName.value} multi-asset extensions.`,
          requestParams: [
            ['model', 'string, required', 'Video model, such as sora-2.'],
            ['prompt', 'string, required', 'Video description.'],
            ['input_reference', 'file | object', 'Optional reference image.'],
            [
              'content[]',
              'array',
              `${siteName.value} extension for one or more image, video, or audio references.`
            ],
            ['size', 'string', 'Video size.'],
            [
              'seconds / duration',
              'string | number',
              `Video duration; duration is a ${siteName.value} extension alias.`
            ],
            [
              'ratio / resolution',
              'string',
              `${siteName.value} extensions for aspect ratio and output resolution. Available values depend on the model.`
            ],
            [
              'generate_audio',
              'boolean',
              `${siteName.value} extension requesting generated audio when supported by the model.`
            ]
          ],
          responseFields: [
            ['id', 'string', 'Video task ID.'],
            ['status', 'string', 'Task status.'],
            ['progress', 'number', 'Task progress.'],
            ['error', 'object | null', 'Failure information.']
          ],
          samples: videoCreateSamples.value
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
          ],
          samples: videoRetrieveSamples
        }
      ],
      openAiAssetInterfaces: [
        {
          title: 'Assets Create',
          method: 'POST',
          path: openAiBaseUrl.value + '/assets',
          description:
            'Create a reusable image, video, or audio asset from a public URL. Assets are shared within a project and upstream asset IDs are never exposed.',
          requestParams: [
            ['model', 'string, required', 'A video model that supports assets.'],
            [
              'type',
              'image | video | audio, required',
              'Asset type; it must match the video reference field used later.'
            ],
            [
              'url',
              'string, required',
              'Public HTTP/HTTPS URL. Base64, data URLs, multipart uploads, and local addresses are not supported.'
            ],
            ['name', 'string', 'Optional name, limited to 50 Unicode characters.']
          ],
          responseFields: [
            ['id', 'string', `${siteName.value} asset ID in asset_* format.`],
            ['type', 'string', 'Asset type.'],
            ['url', 'string', 'Public URL submitted during creation.'],
            ['name', 'string | null', 'Asset name.'],
            ['status', 'string', 'processing, active, failed, expired, or deleted.'],
            ['error', 'string', 'Returned only when processing fails.']
          ],
          samples: assetCreateSamples.value
        },
        {
          title: 'Assets Retrieve',
          method: 'GET',
          path: openAiBaseUrl.value + '/assets/{asset_id}',
          description:
            'Retrieve an asset and refresh its processing status. Access is isolated to the current project.',
          requestParams: [
            ['asset_id', 'string, required', 'The asset_* ID returned by asset creation.']
          ],
          responseFields: [
            ['id', 'string', `${siteName.value} asset ID.`],
            ['status', 'string', 'processing, active, failed, expired, or deleted.'],
            ['error', 'string', 'Returned only when processing fails.']
          ],
          samples: assetRetrieveSamples.value
        }
      ],
      openAiAudioInterfaces: [
        {
          title: 'Audio Transcriptions',
          method: 'POST',
          path: `${openAiBaseUrl.value}/audio/transcriptions`,
          description: 'Upload audio and return the transcribed text.',
          requestParams: [
            ['file', 'file, required', 'Audio file to transcribe.'],
            ['model', 'string, required', 'Audio transcription model.'],
            [
              'language / languages[]',
              'string | array',
              'Language hint; these fields are mutually exclusive.'
            ],
            [
              'prompt / keywords[]',
              'string | array',
              'Fun-ASR-Flash context, limited to 400 characters.'
            ],
            [
              'response_format',
              'json | text | verbose_json | srt | vtt',
              'Response format; defaults to json.'
            ],
            [
              'timestamp_granularities[]',
              'word | segment',
              'Word or segment timestamps for verbose_json.'
            ],
            ['stream', 'boolean', 'Fun-ASR-Flash transcript.text.delta/done SSE stream.'],
            ['temperature', 'number', 'Only 0 is supported by Fun-ASR-Flash.']
          ],
          responseFields: [
            ['text', 'string', 'Recognized text content.'],
            ['segments / words', 'array', 'Segment and word timestamps in verbose_json.'],
            ['usage', 'object', 'Usage measured in audio seconds.']
          ],
          samples: audioSamples
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
          ],
          samples: embeddingsSamples.value
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
          ],
          samples: modelsSamples
        }
      ],
      videoWorkflowTitle: 'Video workflow',
      videoWorkflowItems: [
        [
          'Prepare assets (optional)',
          'Call /v1/assets and poll until active. Creating assets does not incur video generation charges.'
        ],
        [
          'Create task',
          `Call /v1/videos. Use input_reference for one image or the ${siteName.value} content[] extension for multiple references.`
        ],
        [
          'Check status',
          'Use /v1/videos/{video_id} to check status, progress, and failure details.'
        ],
        [
          'Download content',
          'When completed, call /v1/videos/{video_id}/content to download the MP4 file.'
        ]
      ],
      videoNotes: [
        [
          'Reference upload',
          `OpenAI-compatible requests upload one image through multipart input_reference. ${siteName.value} JSON requests can use public URLs or asset://asset_* references in content[].`
        ],
        [
          'Multiple references',
          'content[] supports image, video, and audio inputs. Limits, role combinations, and media capabilities depend on the selected model and channel.'
        ],
        [
          'Asset constraints',
          'asset:// references must belong to the current project, match the field type, and be active. Assets in one request must resolve to the same available upstream binding.'
        ],
        [
          'Task status',
          'The status can be queued, in_progress, completed, or failed; inspect error when a task fails.'
        ]
      ],
      endpointHeaders: ['Module', 'Method', 'API path', 'Description', 'Status'],
      openAiIntro: `All APIs use Bearer Token auth. Set the Base URL to the ${siteName.value} /v1 URL. The table includes OpenAI-compatible APIs and ${siteName.value} extensions; extensions are explicitly identified in the status or description.`,
      openAiAuthItems: [
        ['Base URL', openAiBaseUrl.value],
        ['Auth header', 'Authorization: Bearer YOUR_API_KEY'],
        [
          'Content-Type',
          'application/json; image, video, and audio upload APIs use multipart/form-data'
        ]
      ],
      endpointSearchPlaceholder: 'Filter endpoints…',
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
        [
          'max_output_tokens',
          'integer',
          'Limits the maximum output tokens for the background task.'
        ],
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
        [
          'error',
          'object | null',
          'On failure, includes code and message; usually null on success.'
        ],
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
        [
          'partial_images',
          'integer',
          'Number of partial preview images requested during streaming.'
        ]
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
      openAiImageAsync: `Background image tasks are created through the Responses image_generation tool, not through a background mode on the Images API itself. ${siteName.value} extends the request with image_format so async image results can return base64 or a URL. Use it for async text-to-image and image-to-image, then retrieve, resume streaming, or cancel through Responses.`,
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
        [
          'tools[].background',
          'string',
          'Background mode, such as transparent or opaque, subject to model and upstream support.'
        ],
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
        [
          'error',
          'object | null',
          'On failure, includes code and message; usually null on success.'
        ],
        [
          'usage',
          'object | null',
          `Final usage data used by ${siteName.value} for records and settlement.`
        ]
      ],
      openAiVideo: `The Videos API follows OpenAI's asynchronous job model, with ${siteName.value} extensions for reusable assets and multiple references. Create returns a video job, retrieve polls status, and content downloads the completed video. Create, retrieve, and content download are supported; list, delete, edits, extensions, and remix are not currently supported.`,
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
          'OpenAI-compatible single reference image. JSON accepts image_url, or multipart can upload one image. file_id is not currently supported.'
        ],
        [
          'content[]',
          'array',
          `${siteName.value} extension using image_url, video_url, or audio_url for multiple public URLs or asset://asset_* references, with role describing each input.`
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
        [
          'duration',
          'integer',
          `${siteName.value} extension and alias for seconds. Available range depends on the model and channel.`
        ],
        [
          'ratio',
          'string',
          `${siteName.value} extension for aspect ratio, such as 16:9, 9:16, or 1:1.`
        ],
        [
          'resolution',
          'string',
          `${siteName.value} extension for output resolution, such as 480p, 720p, or 1080p.`
        ],
        [
          'generate_audio',
          'boolean',
          `${siteName.value} extension requesting synchronized audio from supported models.`
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
        [
          'seconds',
          'string',
          'Output duration; extensions may return the stitched total duration.'
        ],
        [
          'expires_at',
          'integer | null',
          'Downloadable asset expiration time when returned by the upstream.'
        ],
        [
          'error',
          'object | null',
          'On failure, includes code and message; usually null on success.'
        ]
      ],
      openAiAudio:
        'The audio transcription endpoint accepts multipart/form-data uploads and returns an OpenAI-compatible transcription result.',
      openAiEmbeddings: `Embeddings are forwarded with the official OpenAI JSON request body and are useful for RAG, semantic search, deduplication, and retrieval. The requested model still uses ${siteName.value} model permissions, routing, billing, and usage records.`
    }
  })

  return {
    content,
    quickStart,
    pythonInstall,
    python,
    nodeInstall,
    node,
    openAiEndpoints
  }
}
