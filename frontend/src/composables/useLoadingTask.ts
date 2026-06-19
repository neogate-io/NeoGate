import type { Ref } from 'vue'

export async function withLoading<T>(loading: Ref<boolean>, task: () => Promise<T>) {
  loading.value = true
  try {
    return await task()
  } finally {
    loading.value = false
  }
}
