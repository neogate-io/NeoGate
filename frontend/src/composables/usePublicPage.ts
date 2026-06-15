import { ElMessage } from 'element-plus'
import { useLocale } from './useLocale'
import { copyTextToClipboard } from '../utils/clipboard'

/** Smooth-scroll to a section by element id. */
export function useScrollTo() {
  return (id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }
}

/** Copy text to clipboard and show a success toast. */
export function useCopyText() {
  const { t } = useLocale()
  return async (text: string) => {
    await copyTextToClipboard(text)
    ElMessage.success(t('apiKeyCopied'))
  }
}
