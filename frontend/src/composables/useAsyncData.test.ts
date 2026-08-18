import { describe, expect, it, vi } from 'vitest'

vi.mock('element-plus/es/components/message/index', () => ({
  ElMessage: { error: vi.fn() }
}))

import { createAsyncData } from './useAsyncData'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

describe('createAsyncData', () => {
  it('closes foreground loading when a newer silent request is running', async () => {
    const foreground = deferred<string>()
    const silent = deferred<string>()
    const loader = vi
      .fn()
      .mockReturnValueOnce(foreground.promise)
      .mockReturnValueOnce(silent.promise)
    const state = createAsyncData(loader, '')

    const foregroundReload = state.reload()
    const silentReload = state.reload({ silent: true })

    foreground.resolve('stale')
    await foregroundReload
    expect(state.loading.value).toBe(false)
    expect(state.data.value).toBe('')

    silent.resolve('latest')
    await silentReload
    expect(state.data.value).toBe('latest')
    expect(state.loaded.value).toBe(true)
  })
})
