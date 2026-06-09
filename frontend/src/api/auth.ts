import { publicRequest } from './request'

export type LoginRole = 'admin' | 'user'
export type LoginResponse = {
  token: string
  role: LoginRole
  requires_password_change?: boolean
}

export function login(username: string, password: string, verificationCode = '') {
  return publicRequest<LoginResponse>('/api/login', {
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
