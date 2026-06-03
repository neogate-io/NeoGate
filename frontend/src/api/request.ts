import { useAuthStore } from '../stores/auth'
import { ApiError } from '../utils/errors'
import { router } from '../router'

export type ApiRequest = <T>(path: string, init?: RequestInit) => Promise<T>

export const publicRequest: ApiRequest = (path, init) => request(path, init)

export async function adminRequest<T>(path: string, init?: RequestInit) {
  return authedRequest<T>(path, init)
}

export async function userRequest<T>(path: string, init?: RequestInit) {
  return authedRequest<T>(path, init)
}

async function authedRequest<T>(path: string, init?: RequestInit) {
  const auth = useAuthStore()

  try {
    return await request<T>(path, init, auth.token)
  } catch (err) {
    if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
      auth.clearToken()
      void router.replace({
        name: 'login',
        query: { redirect: router.currentRoute.value.fullPath }
      }).catch(() => undefined)
    }

    throw err
  }
}

export async function adminUploadRequest<T>(path: string, form: FormData, init: RequestInit = {}) {
  const auth = useAuthStore()
  const headers = new Headers(init.headers)
  if (auth.token) headers.set('authorization', `Bearer ${auth.token}`)

  const response = await fetch(path, { ...init, method: init.method ?? 'POST', body: form, headers })
  const data = await response.json().catch(() => ({}))

  if (!response.ok) {
    throw new ApiError(readErrorMessage(data) ?? response.statusText, response.status)
  }

  return data as T
}

async function request<T>(path: string, init: RequestInit = {}, token = ''): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('content-type', 'application/json')

  if (token) headers.set('authorization', `Bearer ${token}`)

  const response = await fetch(path, { ...init, headers })
  const data = await response.json().catch(() => ({}))

  if (!response.ok) {
    throw new ApiError(readErrorMessage(data) ?? response.statusText, response.status)
  }

  return data as T
}

function readErrorMessage(data: unknown) {
  if (typeof data === 'object' && data && 'error' in data) {
    const error = (data as { error?: unknown }).error
    if (typeof error === 'string') return error
  }
}
