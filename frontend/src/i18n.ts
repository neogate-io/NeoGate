import { adminMessages } from './i18n/admin'
import { appMessages } from './i18n/apps'
import { publicMessages } from './i18n/public'
import { userMessages } from './i18n/user'

export const defaultLocale = 'zh-CN'

export const locales = [
  { code: 'zh-CN', label: '简体中文' },
  { code: 'en-US', label: 'English' }
] as const

export type Locale = (typeof locales)[number]['code']

const messages = {
  'zh-CN': {
    ...publicMessages['zh-CN'],
    ...adminMessages['zh-CN'],
    ...appMessages['zh-CN'],
    ...userMessages['zh-CN']
  },
  'en-US': {
    ...publicMessages['en-US'],
    ...adminMessages['en-US'],
    ...appMessages['en-US'],
    ...userMessages['en-US']
  }
} satisfies Record<Locale, Record<string, string>>

export type MessageKey = keyof (typeof messages)[Locale]
export type TranslateParams = Record<string, unknown>

export function isLocale(value: string | null): value is Locale {
  return locales.some((locale) => locale.code === value)
}

export function isMessageKey(value: unknown): value is MessageKey {
  return typeof value === 'string' && value in messages[defaultLocale]
}

export function translate(locale: Locale, key: MessageKey, params?: TranslateParams) {
  const message = messages[locale][key]
  if (!params) return message

  return message.replace(/\{(\w+)\}/g, (placeholder, name) => {
    const value = params[name]
    return value == null ? placeholder : String(value)
  })
}
