import { ref } from 'vue'
import { describe, expect, it } from 'vitest'
import { createLatestTask } from './useLatestTask'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

describe('createLatestTask', () => {
  it('applies only the latest response and keeps loading until it finishes', async () => {
    const loading = ref(false)
    const latestTask = createLatestTask(loading)
    const first = deferred<string>()
    const second = deferred<string>()
    const applied: string[] = []

    const firstRun = latestTask.run(
      () => first.promise,
      (value) => applied.push(value)
    )
    const secondRun = latestTask.run(
      () => second.promise,
      (value) => applied.push(value)
    )
    first.resolve('old')
    expect(await firstRun).toBe(false)
    expect(loading.value).toBe(true)

    second.resolve('current')
    expect(await secondRun).toBe(true)
    expect(applied).toEqual(['current'])
    expect(loading.value).toBe(false)
  })

  it('suppresses errors from stale requests', async () => {
    const latestTask = createLatestTask()
    const first = deferred<string>()
    const second = deferred<string>()

    const firstRun = latestTask.run(
      () => first.promise,
      () => undefined
    )
    const secondRun = latestTask.run(
      () => second.promise,
      () => undefined
    )
    first.reject(new Error('stale'))
    second.resolve('current')

    await expect(firstRun).resolves.toBe(false)
    await expect(secondRun).resolves.toBe(true)
  })
})
