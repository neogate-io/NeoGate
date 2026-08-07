import { onBeforeUnmount, type Ref } from 'vue'

type ApplyResult<T> = (value: T) => void

export function createLatestTask(loading?: Ref<boolean>) {
  let requestId = 0
  let disposed = false

  async function run<T>(task: () => Promise<T>, apply: ApplyResult<T>) {
    const currentRequest = ++requestId
    if (loading) loading.value = true

    try {
      const value = await task()
      if (disposed || currentRequest !== requestId) return false
      apply(value)
      return true
    } catch (error) {
      if (disposed || currentRequest !== requestId) return false
      throw error
    } finally {
      if (!disposed && currentRequest === requestId && loading) {
        loading.value = false
      }
    }
  }

  function invalidate() {
    requestId += 1
    if (loading) loading.value = false
  }

  function dispose() {
    disposed = true
    invalidate()
  }

  return { run, invalidate, dispose }
}

export function useLatestTask(loading?: Ref<boolean>) {
  const latestTask = createLatestTask(loading)
  onBeforeUnmount(latestTask.dispose)
  return latestTask
}

export type LatestTask = ReturnType<typeof useLatestTask>
