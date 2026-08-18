import { describe, expect, it, vi } from 'vitest'
import { abortableDelay, isAbortError } from './async'

describe('abortableDelay', () => {
  it('resolves after the requested delay', async () => {
    vi.useFakeTimers()
    const delay = abortableDelay(100)

    await vi.advanceTimersByTimeAsync(100)

    await expect(delay).resolves.toBeUndefined()
    vi.useRealTimers()
  })

  it('rejects immediately when aborted', async () => {
    const controller = new AbortController()
    const delay = abortableDelay(100, controller.signal)
    const result = delay.catch((error: unknown) => error)

    controller.abort()

    const error = await result
    expect(error).toMatchObject({ name: 'AbortError' })
    expect(isAbortError(error)).toBe(true)
  })
})
