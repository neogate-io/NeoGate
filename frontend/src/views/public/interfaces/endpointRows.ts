/**
 * Single source of truth for the endpoint overview tables on the public
 * interfaces docs page. Both locales are rendered from these rows; the
 * composables map them to display rows via toEndpointDisplayRows().
 */
export interface EndpointRow {
  name: string // Module name, English in both locales
  method: string
  path: string
  supported: boolean
  statusNote?: { zh: string; en: string } // Status suffix; use {site} for the brand name
  anchor?: string
  description: { zh: string; en: string }
}

export interface EndpointDisplayRow {
  name: string
  method: string
  path: string
  description: string
  status: string
  supported: boolean
  anchor?: string
}

export function toEndpointDisplayRows(
  rows: EndpointRow[],
  locale: 'zh' | 'en',
  siteName: string
): EndpointDisplayRow[] {
  const isZh = locale === 'zh'
  return rows.map((row) => {
    let status = row.supported
      ? isZh
        ? '已支持'
        : 'Supported'
      : isZh
        ? '暂未支持'
        : 'Not supported'
    if (row.supported && row.statusNote) {
      const note = (isZh ? row.statusNote.zh : row.statusNote.en).replace(/\{site\}/g, siteName)
      status += isZh ? `（${note}）` : ` (${note})`
    }
    return {
      name: row.name,
      method: row.method,
      path: row.path,
      description: isZh ? row.description.zh : row.description.en,
      status,
      supported: row.supported,
      anchor: row.anchor
    }
  })
}

const backgroundTasksNote = { zh: '后台任务', en: 'background tasks' }
const streamingNote = { zh: '含流式', en: 'streaming' }
const siteExtensionNote = { zh: '{site} 扩展', en: '{site} extension' }

export const openAiEndpointRows: EndpointRow[] = [
  {
    name: 'Models',
    method: 'GET',
    path: '/v1/models',
    supported: true,
    anchor: 'models',
    description: {
      zh: '列出当前 API Key 可调用的模型。',
      en: 'List models callable by the current API key.'
    }
  },
  {
    name: 'Models',
    method: 'GET',
    path: '/v1/models/{model}',
    supported: true,
    anchor: 'models',
    description: { zh: '查询单个模型详情。', en: 'Retrieve a single model.' }
  },
  {
    name: 'Models',
    method: 'DELETE',
    path: '/v1/models/{model}',
    supported: false,
    description: { zh: '删除或取消可删除模型。', en: 'Delete or cancel a deletable model.' }
  },
  {
    name: 'Chat Completions',
    method: 'POST',
    path: '/v1/chat/completions',
    supported: true,
    anchor: 'text',
    description: {
      zh: '创建 Chat Completions 文本生成。',
      en: 'Create Chat Completions text generation.'
    }
  },
  {
    name: 'Chat Completions',
    method: 'GET',
    path: '/v1/chat/completions/{completion_id}',
    supported: false,
    description: { zh: '查询已保存的 Chat Completion。', en: 'Retrieve a stored Chat Completion.' }
  },
  {
    name: 'Chat Completions',
    method: 'GET',
    path: '/v1/chat/completions/{completion_id}/messages',
    supported: false,
    description: { zh: '列出已保存对话的消息。', en: 'List messages of a stored conversation.' }
  },
  {
    name: 'Chat Completions',
    method: 'PATCH',
    path: '/v1/chat/completions/{completion_id}',
    supported: false,
    description: {
      zh: '更新已保存对话的元数据。',
      en: 'Update the metadata of a stored conversation.'
    }
  },
  {
    name: 'Chat Completions',
    method: 'DELETE',
    path: '/v1/chat/completions/{completion_id}',
    supported: false,
    description: { zh: '删除已保存的 Chat Completion。', en: 'Delete a stored Chat Completion.' }
  },
  {
    name: 'Responses',
    method: 'POST',
    path: '/v1/responses',
    supported: true,
    anchor: 'text',
    description: {
      zh: '创建 Responses 文本、多模态或后台任务。',
      en: 'Create Responses text, multimodal, or background tasks.'
    }
  },
  {
    name: 'Responses',
    method: 'GET',
    path: '/v1/responses/{response_id}',
    supported: true,
    statusNote: backgroundTasksNote,
    anchor: 'text-async',
    description: {
      zh: '查询 Response 结果或恢复流式读取。',
      en: 'Retrieve a Response result or resume streaming.'
    }
  },
  {
    name: 'Responses',
    method: 'DELETE',
    path: '/v1/responses/{response_id}',
    supported: false,
    description: { zh: '删除已保存的 Response。', en: 'Delete a stored Response.' }
  },
  {
    name: 'Responses',
    method: 'POST',
    path: '/v1/responses/{response_id}/cancel',
    supported: true,
    statusNote: backgroundTasksNote,
    anchor: 'text-async',
    description: { zh: '取消后台 Response 任务。', en: 'Cancel a background Response task.' }
  },
  {
    name: 'Responses',
    method: 'GET',
    path: '/v1/responses/{response_id}/input_items',
    supported: true,
    statusNote: backgroundTasksNote,
    description: { zh: '列出 Response 的输入项。', en: 'List the input items of a Response.' }
  },
  {
    name: 'Images',
    method: 'POST',
    path: '/v1/images/generations',
    supported: true,
    statusNote: streamingNote,
    anchor: 'images',
    description: { zh: '根据提示词生成图片。', en: 'Generate images from prompts.' }
  },
  {
    name: 'Images',
    method: 'POST',
    path: '/v1/images/edits',
    supported: true,
    statusNote: streamingNote,
    anchor: 'images',
    description: { zh: '编辑上传图片或进行图生图。', en: 'Edit uploaded images or image inputs.' }
  },
  {
    name: 'Images',
    method: 'POST',
    path: '/v1/images/variations',
    supported: true,
    anchor: 'images',
    description: {
      zh: '基于输入图片生成变体。',
      en: 'Create variations from an input image.'
    }
  },
  {
    name: 'Videos',
    method: 'POST',
    path: '/v1/videos',
    supported: true,
    anchor: 'videos',
    description: { zh: '创建视频生成任务。', en: 'Create a video generation task.' }
  },
  {
    name: 'Assets',
    method: 'POST',
    path: '/v1/assets',
    supported: true,
    statusNote: siteExtensionNote,
    anchor: 'videos',
    description: {
      zh: '通过公网 URL 创建可复用的图片、视频或音频素材。',
      en: 'Create a reusable image, video, or audio asset from a public URL.'
    }
  },
  {
    name: 'Assets',
    method: 'GET',
    path: '/v1/assets/{asset_id}',
    supported: true,
    statusNote: siteExtensionNote,
    anchor: 'videos',
    description: {
      zh: '查询并刷新素材处理状态。',
      en: 'Retrieve and refresh asset processing status.'
    }
  },
  {
    name: 'Videos',
    method: 'GET',
    path: '/v1/videos',
    supported: false,
    description: { zh: '列出视频任务。', en: 'List video tasks.' }
  },
  {
    name: 'Videos',
    method: 'GET',
    path: '/v1/videos/{video_id}',
    supported: true,
    anchor: 'videos',
    description: { zh: '查询视频任务状态。', en: 'Retrieve video task status.' }
  },
  {
    name: 'Videos',
    method: 'DELETE',
    path: '/v1/videos/{video_id}',
    supported: false,
    description: { zh: '删除视频任务。', en: 'Delete a video task.' }
  },
  {
    name: 'Videos',
    method: 'GET',
    path: '/v1/videos/{video_id}/content',
    supported: true,
    anchor: 'videos',
    description: { zh: '下载生成完成的视频文件。', en: 'Download completed video content.' }
  },
  {
    name: 'Videos',
    method: 'POST',
    path: '/v1/videos/edits',
    supported: false,
    description: { zh: '编辑已有视频。', en: 'Edit an existing video.' }
  },
  {
    name: 'Videos',
    method: 'POST',
    path: '/v1/videos/extensions',
    supported: false,
    description: {
      zh: '扩展已有视频时长或内容。',
      en: 'Extend the duration or content of an existing video.'
    }
  },
  {
    name: 'Videos',
    method: 'POST',
    path: '/v1/videos/{video_id}/remix',
    supported: false,
    description: {
      zh: '基于已有视频重新生成版本。',
      en: 'Regenerate a new version from an existing video.'
    }
  },
  {
    name: 'Embeddings',
    method: 'POST',
    path: '/v1/embeddings',
    supported: true,
    anchor: 'embeddings',
    description: { zh: '创建文本向量嵌入。', en: 'Create text embeddings.' }
  },
  {
    name: 'Audio',
    method: 'POST',
    path: '/v1/audio/speech',
    supported: false,
    description: { zh: '将文本转换为语音。', en: 'Convert text to speech.' }
  },
  {
    name: 'Audio',
    method: 'POST',
    path: '/v1/audio/transcriptions',
    supported: true,
    anchor: 'audio',
    description: { zh: '将音频转写为文本。', en: 'Transcribe uploaded audio to text.' }
  },
  {
    name: 'Audio',
    method: 'POST',
    path: '/v1/audio/translations',
    supported: false,
    description: { zh: '将音频翻译为文本。', en: 'Translate audio to text.' }
  },
  {
    name: 'Moderations',
    method: 'POST',
    path: '/v1/moderations',
    supported: true,
    description: { zh: '对输入内容进行安全审核。', en: 'Moderate input content.' }
  },
  {
    name: 'Files',
    method: 'POST',
    path: '/v1/files',
    supported: false,
    description: { zh: '上传文件资源。', en: 'Upload a file resource.' }
  },
  {
    name: 'Files',
    method: 'GET',
    path: '/v1/files',
    supported: false,
    description: { zh: '列出已上传文件。', en: 'List uploaded files.' }
  },
  {
    name: 'Files',
    method: 'GET',
    path: '/v1/files/{file_id}',
    supported: false,
    description: { zh: '查询文件元数据。', en: 'Retrieve file metadata.' }
  },
  {
    name: 'Files',
    method: 'DELETE',
    path: '/v1/files/{file_id}',
    supported: false,
    description: { zh: '删除文件。', en: 'Delete a file.' }
  },
  {
    name: 'Files',
    method: 'GET',
    path: '/v1/files/{file_id}/content',
    supported: false,
    description: { zh: '下载文件内容。', en: 'Download file content.' }
  },
  {
    name: 'Uploads',
    method: 'POST',
    path: '/v1/uploads',
    supported: false,
    description: { zh: '创建分片上传会话。', en: 'Create a multipart upload session.' }
  },
  {
    name: 'Uploads',
    method: 'POST',
    path: '/v1/uploads/{upload_id}/parts',
    supported: false,
    description: { zh: '上传一个文件分片。', en: 'Upload a file part.' }
  },
  {
    name: 'Uploads',
    method: 'POST',
    path: '/v1/uploads/{upload_id}/complete',
    supported: false,
    description: { zh: '完成分片上传。', en: 'Complete a multipart upload.' }
  },
  {
    name: 'Uploads',
    method: 'POST',
    path: '/v1/uploads/{upload_id}/cancel',
    supported: false,
    description: { zh: '取消分片上传。', en: 'Cancel a multipart upload.' }
  },
  {
    name: 'Batches',
    method: 'POST',
    path: '/v1/batches',
    supported: false,
    description: { zh: '创建 OpenAI 批量任务。', en: 'Create an OpenAI batch task.' }
  },
  {
    name: 'Batches',
    method: 'GET',
    path: '/v1/batches',
    supported: false,
    description: { zh: '列出批量任务。', en: 'List batch tasks.' }
  },
  {
    name: 'Batches',
    method: 'GET',
    path: '/v1/batches/{batch_id}',
    supported: false,
    description: { zh: '查询批量任务。', en: 'Retrieve a batch task.' }
  },
  {
    name: 'Batches',
    method: 'POST',
    path: '/v1/batches/{batch_id}/cancel',
    supported: false,
    description: { zh: '取消批量任务。', en: 'Cancel a batch task.' }
  },
  {
    name: 'Fine-tuning',
    method: 'POST',
    path: '/v1/fine_tuning/jobs',
    supported: false,
    description: { zh: '创建微调任务。', en: 'Create a fine-tuning job.' }
  },
  {
    name: 'Fine-tuning',
    method: 'GET',
    path: '/v1/fine_tuning/jobs',
    supported: false,
    description: { zh: '列出微调任务。', en: 'List fine-tuning jobs.' }
  },
  {
    name: 'Fine-tuning',
    method: 'GET',
    path: '/v1/fine_tuning/jobs/{fine_tuning_job_id}',
    supported: false,
    description: { zh: '查询微调任务。', en: 'Retrieve a fine-tuning job.' }
  },
  {
    name: 'Fine-tuning',
    method: 'POST',
    path: '/v1/fine_tuning/jobs/{fine_tuning_job_id}/cancel',
    supported: false,
    description: { zh: '取消微调任务。', en: 'Cancel a fine-tuning job.' }
  },
  {
    name: 'Fine-tuning',
    method: 'GET',
    path: '/v1/fine_tuning/jobs/{fine_tuning_job_id}/events',
    supported: false,
    description: { zh: '列出微调事件。', en: 'List fine-tuning events.' }
  },
  {
    name: 'Fine-tuning',
    method: 'GET',
    path: '/v1/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints',
    supported: false,
    description: { zh: '列出微调检查点。', en: 'List fine-tuning checkpoints.' }
  },
  {
    name: 'Vector Stores',
    method: 'POST',
    path: '/v1/vector_stores',
    supported: false,
    description: { zh: '创建向量库。', en: 'Create a vector store.' }
  },
  {
    name: 'Vector Stores',
    method: 'GET',
    path: '/v1/vector_stores',
    supported: false,
    description: { zh: '列出向量库。', en: 'List vector stores.' }
  },
  {
    name: 'Vector Stores',
    method: 'GET/PATCH/DELETE',
    path: '/v1/vector_stores/{vector_store_id}',
    supported: false,
    description: {
      zh: '查询、更新或删除向量库。',
      en: 'Retrieve, update, or delete a vector store.'
    }
  },
  {
    name: 'Vector Store Files',
    method: 'POST/GET',
    path: '/v1/vector_stores/{vector_store_id}/files',
    supported: false,
    description: { zh: '添加或列出向量库文件。', en: 'Add or list vector store files.' }
  },
  {
    name: 'Vector Store Files',
    method: 'GET/DELETE',
    path: '/v1/vector_stores/{vector_store_id}/files/{file_id}',
    supported: false,
    description: {
      zh: '查询或移除向量库文件。',
      en: 'Retrieve or remove a vector store file.'
    }
  },
  {
    name: 'Vector Store File Batches',
    method: 'POST/GET',
    path: '/v1/vector_stores/{vector_store_id}/file_batches',
    supported: false,
    description: { zh: '创建向量库文件批处理。', en: 'Create a vector store file batch.' }
  },
  {
    name: 'Vector Store File Batches',
    method: 'GET/POST',
    path: '/v1/vector_stores/{vector_store_id}/file_batches/{batch_id}',
    supported: false,
    description: { zh: '查询或取消文件批处理。', en: 'Retrieve or cancel a file batch.' }
  },
  {
    name: 'Assistants',
    method: 'POST/GET',
    path: '/v1/assistants',
    supported: false,
    description: { zh: '创建或列出 Assistant。', en: 'Create or list Assistants.' }
  },
  {
    name: 'Assistants',
    method: 'GET/PATCH/DELETE',
    path: '/v1/assistants/{assistant_id}',
    supported: false,
    description: {
      zh: '查询、更新或删除 Assistant。',
      en: 'Retrieve, update, or delete an Assistant.'
    }
  },
  {
    name: 'Threads',
    method: 'POST',
    path: '/v1/threads',
    supported: false,
    description: { zh: '创建 Assistants 线程。', en: 'Create an Assistants thread.' }
  },
  {
    name: 'Threads',
    method: 'GET/PATCH/DELETE',
    path: '/v1/threads/{thread_id}',
    supported: false,
    description: { zh: '查询、更新或删除线程。', en: 'Retrieve, update, or delete a thread.' }
  },
  {
    name: 'Thread Messages',
    method: 'POST/GET',
    path: '/v1/threads/{thread_id}/messages',
    supported: false,
    description: { zh: '创建或列出线程消息。', en: 'Create or list thread messages.' }
  },
  {
    name: 'Thread Messages',
    method: 'GET/PATCH/DELETE',
    path: '/v1/threads/{thread_id}/messages/{message_id}',
    supported: false,
    description: {
      zh: '查询、更新或删除线程消息。',
      en: 'Retrieve, update, or delete a thread message.'
    }
  },
  {
    name: 'Thread Runs',
    method: 'POST/GET',
    path: '/v1/threads/{thread_id}/runs',
    supported: false,
    description: { zh: '创建或列出线程运行。', en: 'Create or list thread runs.' }
  },
  {
    name: 'Thread Runs',
    method: 'GET/PATCH',
    path: '/v1/threads/{thread_id}/runs/{run_id}',
    supported: false,
    description: { zh: '查询或更新线程运行。', en: 'Retrieve or update a thread run.' }
  },
  {
    name: 'Thread Runs',
    method: 'POST',
    path: '/v1/threads/{thread_id}/runs/{run_id}/cancel',
    supported: false,
    description: { zh: '取消线程运行。', en: 'Cancel a thread run.' }
  },
  {
    name: 'Thread Runs',
    method: 'POST',
    path: '/v1/threads/{thread_id}/runs/{run_id}/submit_tool_outputs',
    supported: false,
    description: { zh: '提交工具调用结果。', en: 'Submit tool call outputs.' }
  },
  {
    name: 'Realtime',
    method: 'POST',
    path: '/v1/realtime/sessions',
    supported: false,
    description: {
      zh: '创建实时语音/多模态会话。',
      en: 'Create a realtime voice/multimodal session.'
    }
  },
  {
    name: 'Realtime',
    method: 'POST',
    path: '/v1/realtime/transcription_sessions',
    supported: false,
    description: { zh: '创建实时转写会话。', en: 'Create a realtime transcription session.' }
  },
  {
    name: 'Realtime',
    method: 'GET (WebSocket)',
    path: '/v1/realtime?model=…',
    supported: true,
    description: {
      zh: '建立实时语音识别 WebSocket 会话（当前对接 qwen3-asr-flash-realtime，按音频时长计费）。',
      en: 'Open a realtime speech-transcription WebSocket session (currently backed by qwen3-asr-flash-realtime, billed per audio second).'
    }
  },
  {
    name: 'Evals',
    method: 'POST/GET',
    path: '/v1/evals',
    supported: false,
    description: { zh: '创建或列出评测。', en: 'Create or list evals.' }
  },
  {
    name: 'Evals',
    method: 'GET/PATCH/DELETE',
    path: '/v1/evals/{eval_id}',
    supported: false,
    description: { zh: '查询、更新或删除评测。', en: 'Retrieve, update, or delete an eval.' }
  },
  {
    name: 'Eval Runs',
    method: 'POST/GET',
    path: '/v1/evals/{eval_id}/runs',
    supported: false,
    description: { zh: '创建或列出评测运行。', en: 'Create or list eval runs.' }
  },
  {
    name: 'Eval Runs',
    method: 'GET/DELETE',
    path: '/v1/evals/{eval_id}/runs/{run_id}',
    supported: false,
    description: { zh: '查询或删除评测运行。', en: 'Retrieve or delete an eval run.' }
  }
]

export const anthropicEndpointRows: EndpointRow[] = [
  {
    name: 'Messages',
    method: 'POST',
    path: '/anthropic/v1/messages',
    supported: true,
    anchor: 'text',
    description: {
      zh: '创建 Anthropic Messages 文本生成。',
      en: 'Create Anthropic Messages text generation.'
    }
  },
  {
    name: 'Messages',
    method: 'POST',
    path: '/anthropic/v1/messages/count_tokens',
    supported: true,
    description: {
      zh: '预估 Messages 请求的 token 数。',
      en: 'Estimate token count for a Messages request.'
    }
  },
  {
    name: 'Message Batches',
    method: 'POST',
    path: '/v1/messages/batches',
    supported: true,
    anchor: 'batches',
    description: { zh: '创建 Message Batch 批量任务。', en: 'Create a Message Batch task.' }
  },
  {
    name: 'Message Batches',
    method: 'GET',
    path: '/v1/messages/batches',
    supported: true,
    anchor: 'batches',
    description: { zh: '列出 Message Batch 批量任务。', en: 'List Message Batch tasks.' }
  },
  {
    name: 'Message Batches',
    method: 'GET',
    path: '/v1/messages/batches/{message_batch_id}',
    supported: true,
    anchor: 'batches',
    description: { zh: '查询单个批量任务状态。', en: 'Retrieve a single batch task.' }
  },
  {
    name: 'Message Batches',
    method: 'POST',
    path: '/v1/messages/batches/{message_batch_id}/cancel',
    supported: true,
    anchor: 'batches',
    description: { zh: '取消批量任务。', en: 'Cancel a batch task.' }
  },
  {
    name: 'Message Batches',
    method: 'DELETE',
    path: '/v1/messages/batches/{message_batch_id}',
    supported: true,
    anchor: 'batches',
    description: { zh: '删除批量任务。', en: 'Delete a batch task.' }
  },
  {
    name: 'Message Batches',
    method: 'GET',
    path: '/v1/messages/batches/{message_batch_id}/results',
    supported: true,
    anchor: 'batches',
    description: { zh: '读取批量任务结果。', en: 'Read batch task results.' }
  },
  {
    name: 'Models',
    method: 'GET',
    path: '/anthropic/v1/models',
    supported: true,
    anchor: 'models',
    description: { zh: '列出 Anthropic 官方模型。', en: 'List official Anthropic models.' }
  },
  {
    name: 'Models',
    method: 'GET',
    path: '/anthropic/v1/models/{model_id}',
    supported: true,
    anchor: 'models',
    description: {
      zh: '查询单个 Anthropic 官方模型。',
      en: 'Retrieve an official Anthropic model.'
    }
  },
  {
    name: 'Files',
    method: 'POST',
    path: '/anthropic/v1/files',
    supported: false,
    description: { zh: '上传文件资源。', en: 'Upload a file resource.' }
  },
  {
    name: 'Files',
    method: 'GET',
    path: '/anthropic/v1/files',
    supported: false,
    description: { zh: '列出文件资源。', en: 'List file resources.' }
  },
  {
    name: 'Files',
    method: 'GET',
    path: '/anthropic/v1/files/{file_id}',
    supported: false,
    description: { zh: '查询文件元数据。', en: 'Retrieve file metadata.' }
  },
  {
    name: 'Files',
    method: 'DELETE',
    path: '/anthropic/v1/files/{file_id}',
    supported: false,
    description: { zh: '删除文件资源。', en: 'Delete a file resource.' }
  },
  {
    name: 'Files',
    method: 'GET',
    path: '/anthropic/v1/files/{file_id}/content',
    supported: false,
    description: { zh: '下载文件内容。', en: 'Download file content.' }
  }
]
