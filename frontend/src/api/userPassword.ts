import { userRequest } from './request'

export function updateUserPassword(payload: { current_password: string; new_password: string }) {
  return userRequest<{ ok: boolean }>('/api/user/password', {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}
