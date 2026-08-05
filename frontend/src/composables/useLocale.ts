import { ref, watchEffect } from 'vue'
import {
  defaultLocale,
  isLocale,
  translate,
  type Locale,
  type MessageKey,
  type TranslateParams
} from '../i18n'

const localeStorageKey = 'neogate_locale'
const storedLocale =
  typeof localStorage === 'undefined' ? null : localStorage.getItem(localeStorageKey)
export const locale = ref<Locale>(isLocale(storedLocale) ? storedLocale : defaultLocale)

export function useLocale() {
  function t(key: MessageKey, params?: TranslateParams) {
    return translate(locale.value, key, params)
  }

  function toggleLocale() {
    locale.value = locale.value === 'zh-CN' ? 'en-US' : 'zh-CN'
  }

  return {
    locale,
    t,
    toggleLocale
  }
}

watchEffect(() => {
  if (typeof document !== 'undefined') {
    document.documentElement.lang = locale.value
  }

  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(localeStorageKey, locale.value)
  }
})
