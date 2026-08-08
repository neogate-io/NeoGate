import { ElMessageBox } from 'element-plus'

type MessageBoxConfirmArgs = Parameters<typeof ElMessageBox.confirm>
type MessageBoxConfirmOptions = NonNullable<MessageBoxConfirmArgs[2]>

type AppConfirmOptions = {
  cancelText: string
  confirmText: string
  type?: MessageBoxConfirmOptions['type']
  danger?: boolean
}

export async function confirmAction(
  message: MessageBoxConfirmArgs[0],
  title: MessageBoxConfirmArgs[1],
  options: AppConfirmOptions
) {
  try {
    await ElMessageBox.confirm(message, title, {
      cancelButtonText: options.cancelText,
      confirmButtonText: options.confirmText,
      confirmButtonClass: options.danger ? 'el-button--danger' : undefined,
      customClass: 'app-confirm-message-box',
      type: options.type
    })
    return true
  } catch {
    return false
  }
}

export function createConfirmAction(cancelText: string | (() => string)) {
  return (
    message: MessageBoxConfirmArgs[0],
    title: MessageBoxConfirmArgs[1],
    options: Omit<AppConfirmOptions, 'cancelText'>
  ) =>
    confirmAction(message, title, {
      ...options,
      cancelText: typeof cancelText === 'function' ? cancelText() : cancelText
    })
}
