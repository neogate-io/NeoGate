import { ElMessage } from 'element-plus'
import { useLocale } from './useLocale'

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
    await navigator.clipboard.writeText(text)
    ElMessage.success(t('apiKeyCopied'))
  }
}
