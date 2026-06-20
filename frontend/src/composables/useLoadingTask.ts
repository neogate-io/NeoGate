import type { Ref } from 'vue'

export async function withLoading<T>(loading: Ref<boolean>, task: () => Promise<T>) {
  loading.value = true
  try {
    return await task()
  } finally {
    loading.value = false
  }
}

export async function withLoadingValue<T, R>(
  loading: Ref<T>,
  activeValue: T,
  idleValue: T,
  task: () => Promise<R>
) {
  loading.value = activeValue
  try {
    return await task()
  } finally {
    loading.value = idleValue
  }
}
