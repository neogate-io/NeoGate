import { onBeforeUnmount, onMounted, ref, type Ref } from 'vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import { readError } from '../utils/errors'

type ReloadOptions = {
  silent?: boolean
}

export function useAsyncData<T>(loader: () => Promise<T>, initialValue: T) {
  const state = createAsyncData(loader, initialValue)

  onMounted(state.reload)
  onBeforeUnmount(state.dispose)

  return {
    data: state.data,
    loading: state.loading,
    loaded: state.loaded,
    error: state.error,
    reload: state.reload
  }
}

export function createAsyncData<T>(loader: () => Promise<T>, initialValue: T) {
  const data = ref(initialValue) as Ref<T>
  const loading = ref(true)
  const loaded = ref(false)
  const error = ref('')
  let requestId = 0
  let foregroundRequestId: number | null = null
  let disposed = false

  async function reload(options: ReloadOptions = {}) {
    const currentRequest = requestId + 1
    requestId = currentRequest
    const silent = options.silent === true
    if (!silent) {
      foregroundRequestId = currentRequest
      loading.value = true
    }
    error.value = ''

    try {
      const nextData = await loader()
      if (disposed || currentRequest !== requestId) return
      data.value = nextData
    } catch (err) {
      if (disposed || currentRequest !== requestId) return
      error.value = readError(err)
      ElMessage.error(error.value)
    } finally {
      if (!disposed && foregroundRequestId === currentRequest) {
        foregroundRequestId = null
        loading.value = false
      }
      if (!disposed && currentRequest === requestId) loaded.value = true
    }
  }

  function dispose() {
    disposed = true
    requestId += 1
    foregroundRequestId = null
  }

  return {
    data,
    loading,
    loaded,
    error,
    reload,
    dispose
  }
}
