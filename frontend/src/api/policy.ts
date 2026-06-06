import { adminRequest, publicRequest, userRequest } from './request'

export type ServiceMode = 'internal' | 'paid'

export type ServicePolicy = {
  setup_completed: boolean
  service_mode: ServiceMode
  credit_required: boolean
  recharge_enabled: boolean
  updated_at?: string | null
}

export function getSetupStatus() {
  return publicRequest<ServicePolicy>('/api/setup/status')
}

export function completeSetup(serviceMode: ServiceMode) {
  return publicRequest<ServicePolicy>('/api/setup', {
    method: 'POST',
    body: JSON.stringify({ service_mode: serviceMode })
  })
}

export function getUserServicePolicy() {
  return userRequest<ServicePolicy>('/api/user/service-policy')
}

export function getAdminServicePolicy() {
  return adminRequest<ServicePolicy>('/api/admin/settings/service-policy')
}

export function saveAdminServicePolicy(input: { credit_required: boolean }) {
  return adminRequest<ServicePolicy>('/api/admin/settings/service-policy', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}
