import { publicRequest } from './request'

export type LoginRole = 'admin' | 'user'

export function login(username: string, password: string, verificationCode = '') {
  return publicRequest<{ token: string; role: LoginRole }>('/api/login', {
    method: 'POST',
    body: JSON.stringify({ username, password, verification_code: verificationCode })
  })
}

export function requestLoginVerificationCode(email: string, locale: string) {
  return publicRequest<{ ok: boolean }>('/api/login-verification-codes', {
    method: 'POST',
    body: JSON.stringify({ email, locale })
  })
}

export function requestPasswordReset(email: string, locale: string) {
  return publicRequest<{ ok: boolean }>('/api/password-reset-requests', {
    method: 'POST',
    body: JSON.stringify({ email, locale })
  })
}

export function resetPassword(token: string, password: string) {
  return publicRequest<{ ok: boolean }>('/api/password-reset', {
    method: 'POST',
    body: JSON.stringify({ token, password })
  })
}
