import { onBeforeUnmount, onMounted, ref, type Ref } from 'vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import { readError } from '../utils/errors'

export function useAsyncData<T>(loader: () => Promise<T>, initialValue: T) {
  const data = ref(initialValue) as Ref<T>
  const loading = ref(true)
  const loaded = ref(false)
  const error = ref('')
  let requestId = 0
  let disposed = false

  async function reload() {
    const currentRequest = requestId + 1
    requestId = currentRequest
    loading.value = true
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
      if (!disposed && currentRequest === requestId) {
        loaded.value = true
        loading.value = false
      }
    }
  }

  onMounted(reload)
  onBeforeUnmount(() => {
    disposed = true
    requestId += 1
  })

  return {
    data,
    loading,
    loaded,
    error,
    reload
  }
}
