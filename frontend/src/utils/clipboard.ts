import { ElMessage } from 'element-plus'
import { readError } from './errors'

export async function copyTextToClipboard(text: string) {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text)
      return
    } catch {
      // Fall through to the textarea fallback for browsers that expose the API
      // but reject it outside a secure context or after an async action.
    }
  }

  copyTextWithTextarea(text)
}

function copyTextWithTextarea(text: string) {
  if (!document.body) {
    throw new Error('Clipboard is not available')
  }

  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.style.position = 'fixed'
  textarea.style.top = '-9999px'
  textarea.style.left = '-9999px'
  textarea.style.opacity = '0'

  const selection = document.getSelection()
  const previousRange = selection?.rangeCount ? selection.getRangeAt(0) : null

  document.body.appendChild(textarea)
  textarea.select()
  textarea.setSelectionRange(0, textarea.value.length)

  const copied = document.execCommand('copy')
  textarea.remove()

  if (previousRange && selection) {
    selection.removeAllRanges()
    selection.addRange(previousRange)
  }

  if (!copied) {
    throw new Error('Clipboard is not available')
  }
}

export async function copyTextWithMessage(text: string, successMessage: string) {
  try {
    await copyTextToClipboard(text)
    ElMessage.success(successMessage)
    return true
  } catch (err) {
    ElMessage.error(readError(err))
    return false
  }
}
