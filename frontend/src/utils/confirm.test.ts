import { beforeEach, describe, expect, it, vi } from 'vitest'

const { confirm } = vi.hoisted(() => ({
  confirm: vi.fn()
}))

vi.mock('element-plus', () => ({
  ElMessageBox: { confirm }
}))

import { confirmAction } from './confirm'

describe('confirmAction', () => {
  beforeEach(() => {
    confirm.mockReset()
  })

  it('uses the scoped application style and danger button class', async () => {
    confirm.mockResolvedValueOnce('confirm')

    await expect(
      confirmAction('Delete this upstream?', 'Delete', {
        cancelText: 'Cancel',
        confirmText: 'Delete',
        danger: true,
        type: 'warning'
      })
    ).resolves.toBe(true)

    expect(confirm).toHaveBeenCalledWith('Delete this upstream?', 'Delete', {
      cancelButtonText: 'Cancel',
      confirmButtonClass: 'el-button--danger',
      confirmButtonText: 'Delete',
      customClass: 'app-confirm-message-box',
      type: 'warning'
    })
  })

  it('returns false when the confirmation is dismissed', async () => {
    confirm.mockRejectedValueOnce(new Error('cancel'))

    await expect(
      confirmAction('Continue?', 'Confirm', {
        cancelText: 'Cancel',
        confirmText: 'Continue'
      })
    ).resolves.toBe(false)
  })
})
