import { onMounted, ref, type Ref } from 'vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import { readError } from '../utils/errors'

export function useAsyncData<T>(loader: () => Promise<T>, initialValue: T) {
  const data = ref(initialValue) as Ref<T>
  const loading = ref(false)
  const error = ref('')

  async function reload() {
    loading.value = true
    error.value = ''

    try {
      data.value = await loader()
    } catch (err) {
      error.value = readError(err)
      ElMessage.error(error.value)
    } finally {
      loading.value = false
    }
  }

  onMounted(reload)

  return {
    data,
    loading,
    error,
    reload
  }
}
