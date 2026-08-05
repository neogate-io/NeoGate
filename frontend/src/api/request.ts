import { ElMessage } from 'element-plus'
import { useAuthStore } from '../stores/auth'
import { ApiError, readApiErrorPayload } from '../utils/errors'
import { translate } from '../i18n'
import { locale } from '../composables/useLocale'
import { router } from '../router'

export type ApiRequest = <T>(path: string, init?: RequestInit) => Promise<T>

export const publicRequest: ApiRequest = (path, init) => request(path, init)

export async function adminRequest<T>(path: string, init?: RequestInit) {
  return authedRequest<T>(path, init)
}

export async function userRequest<T>(path: string, init?: RequestInit) {
  return authedRequest<T>(path, init)
}

export async function adminFileRequest(path: string, init?: RequestInit) {
  const auth = useAuthStore()
  const headers = new Headers(init?.headers)
  if (auth.token) headers.set('authorization', `Bearer ${auth.token}`)

  try {
    const response = await fetch(path, { ...init, headers })
    if (!response.ok) {
      const data = await response.json().catch(() => ({}))
      const error = readApiErrorPayload(data)
      throw new ApiError(error?.message ?? response.statusText, response.status, error?.code)
    }
    return {
      blob: await response.blob(),
      filename: filenameFromContentDisposition(response.headers.get('content-disposition'))
    }
  } catch (err) {
    handleAuthFailure(err)
    throw err
  }
}

async function authedRequest<T>(path: string, init?: RequestInit) {
  const auth = useAuthStore()

  try {
    return await request<T>(path, init, auth.token)
  } catch (err) {
    handleAuthFailure(err)
    throw err
  }
}

export async function adminUploadRequest<T>(path: string, form: FormData, init: RequestInit = {}) {
  const auth = useAuthStore()
  const headers = new Headers(init.headers)
  if (auth.token) headers.set('authorization', `Bearer ${auth.token}`)

  try {
    return await parseJsonResponse<T>(
      await fetch(path, { ...init, method: init.method ?? 'POST', body: form, headers })
    )
  } catch (err) {
    handleAuthFailure(err)
    throw err
  }
}

async function request<T>(path: string, init: RequestInit = {}, token = ''): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('content-type', 'application/json')

  if (token) headers.set('authorization', `Bearer ${token}`)

  return parseJsonResponse<T>(await fetch(path, { ...init, headers }))
}

async function parseJsonResponse<T>(response: Response): Promise<T> {
  const data = await response.json().catch(() => ({}))

  if (!response.ok) {
    const error = readApiErrorPayload(data)
    throw new ApiError(error?.message ?? response.statusText, response.status, error?.code)
  }

  return data as T
}

function handleAuthFailure(err: unknown) {
  if (!(err instanceof ApiError) || (err.status !== 401 && err.status !== 403)) {
    return
  }

  const auth = useAuthStore()
  if (isPasswordChangeRequiredError(err) && auth.isUser) {
    auth.markPasswordChangeRequired()
    void router
      .replace({
        name: 'changePassword',
        query: { redirect: router.currentRoute.value.fullPath }
      })
      .catch(() => undefined)
    return
  }

  ElMessage.warning(translate(locale.value, 'sessionExpired'))
  auth.clearToken()
  void router
    .replace({
      name: 'login',
      query: { redirect: router.currentRoute.value.fullPath }
    })
    .catch(() => undefined)
}

function isPasswordChangeRequiredError(err: ApiError) {
  return err.code === 'password_change_required'
}

function filenameFromContentDisposition(value: string | null) {
  const match = value?.match(/filename="?([^";]+)"?/i)
  return match?.[1] ?? null
}
