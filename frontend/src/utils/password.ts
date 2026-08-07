import type { MessageKey } from '../i18n'
import { ApiError, readError } from './errors'

export type PasswordChangeForm = {
  currentPassword: string
  newPassword: string
  confirmPassword: string
}

type Translate = (key: MessageKey) => string

type PasswordChangeMessageKeys = {
  mismatchKey?: MessageKey
  sameAsCurrentKey?: MessageKey
  currentIncorrectKey?: MessageKey
}

type PasswordChangeErrorOptions = PasswordChangeMessageKeys & {
  fallback?: 'message' | 'readError'
}

export function resetPasswordChangeForm(form: PasswordChangeForm) {
  form.currentPassword = ''
  form.newPassword = ''
  form.confirmPassword = ''
}

export function readPasswordChangeValidationError(
  form: PasswordChangeForm,
  t: Translate,
  keys: PasswordChangeMessageKeys = {}
) {
  if (!form.currentPassword || !form.newPassword || !form.confirmPassword) {
    return t('passwordRequired')
  }
  if (form.newPassword.length < 8) {
    return t('passwordMinLength')
  }
  if (form.newPassword !== form.confirmPassword) {
    return t(keys.mismatchKey ?? 'passwordMismatch')
  }
  if (form.currentPassword === form.newPassword) {
    return t(keys.sameAsCurrentKey ?? 'passwordSameAsCurrent')
  }
  return ''
}

export function readPasswordChangeError(
  err: unknown,
  t: Translate,
  options: PasswordChangeErrorOptions = {}
) {
  if (err instanceof ApiError && err.code === 'current_password_incorrect') {
    return t(options.currentIncorrectKey ?? 'currentPasswordIncorrect')
  }
  if (err instanceof ApiError && err.code === 'password_min_length') {
    return t('passwordMinLength')
  }
  if (
    options.sameAsCurrentKey &&
    err instanceof ApiError &&
    err.code === 'password_same_as_current'
  ) {
    return t(options.sameAsCurrentKey)
  }
  return options.fallback === 'readError'
    ? readError(err)
    : err instanceof Error
      ? err.message
      : String(err)
}
