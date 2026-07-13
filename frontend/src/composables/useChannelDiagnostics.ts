import { computed, nextTick, ref } from 'vue'
import {
  streamChannelDiagnostic,
  type ChannelDiagnosticScope,
  type ChannelDiagnosticStreamEvent
} from '../api/channels'
import type { Channel, ChannelDiagnosticReport } from '../types/admin'
import { readError } from '../utils/errors'

export function useChannelDiagnostics(onComplete?: () => Promise<void> | void) {
  const dialogOpen = ref(false)
  const report = ref<ChannelDiagnosticReport | null>(null)
  const error = ref('')
  const channel = ref<Channel | null>(null)
  const diagnosingChannelId = ref<number | null>(null)
  const liveSteps = ref<Array<Extract<ChannelDiagnosticStreamEvent, { type: 'model_result' }>>>(
    []
  )
  const currentModel = ref('')
  const liveListRef = ref<HTMLElement | null>(null)
  const inProgress = computed(() => diagnosingChannelId.value !== null)

  function isChannelDiagnosing(channelId: number) {
    return diagnosingChannelId.value === channelId
  }

  async function scrollLiveListToBottom() {
    await nextTick()
    const list = liveListRef.value
    if (list) list.scrollTop = list.scrollHeight
  }

  async function run(row: Channel, scope: ChannelDiagnosticScope = 'all') {
    if (inProgress.value) return

    channel.value = row
    report.value = null
    error.value = ''
    liveSteps.value = []
    currentModel.value = ''
    dialogOpen.value = true
    diagnosingChannelId.value = row.id
    try {
      report.value = await streamChannelDiagnostic(row.id, scope, (event) => {
        if (event.type === 'model_started') {
          currentModel.value = event.model
        }
        if (event.type === 'model_result') {
          liveSteps.value.push(event)
          currentModel.value = ''
          void scrollLiveListToBottom()
        }
        if (event.type === 'finished') {
          report.value = event.report
        }
      })
      await onComplete?.()
    } catch (err) {
      error.value = readError(err)
    } finally {
      diagnosingChannelId.value = null
    }
  }

  return {
    dialogOpen,
    report,
    error,
    channel,
    liveSteps,
    currentModel,
    liveListRef,
    inProgress,
    isChannelDiagnosing,
    run
  }
}

export type UseChannelDiagnostics = ReturnType<typeof useChannelDiagnostics>
