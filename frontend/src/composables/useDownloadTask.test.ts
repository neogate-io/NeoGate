import { describe, expect, it, vi } from 'vitest'

const { showError } = vi.hoisted(() => ({ showError: vi.fn() }))

vi.mock('element-plus', () => ({
  ElMessage: { error: showError }
}))

import { useDownloadTask } from './useDownloadTask'

describe('useDownloadTask', () => {
  it('prevents concurrent downloads', async () => {
    let finish!: () => void
    const pending = new Promise<void>((resolve) => {
      finish = resolve
    })
    const task = useDownloadTask()
    const first = task.run(() => pending)

    expect(task.downloading.value).toBe(true)
    await expect(task.run(async () => undefined)).resolves.toBe(false)

    finish()
    await expect(first).resolves.toBe(true)
    expect(task.downloading.value).toBe(false)
  })

  it('reports failures and restores the idle state', async () => {
    showError.mockClear()
    const task = useDownloadTask()

    await expect(task.run(async () => Promise.reject(new Error('download failed')))).resolves.toBe(
      false
    )

    expect(showError).toHaveBeenCalledWith('download failed')
    expect(task.downloading.value).toBe(false)
  })
})
