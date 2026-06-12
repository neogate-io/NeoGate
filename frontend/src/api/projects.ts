import type { CursorPage, Project, ProjectMember, ProjectStatus } from '../types/admin'
import { adminRequest } from './request'

export type ProjectPage = CursorPage<Project>

export type GetProjectsFilters = {
  search?: string
  status?: ProjectStatus | ''
  limit?: number
  cursor?: string
}

export type CreateProjectPayload = {
  name: string
  owner_user_id: number
  status?: ProjectStatus
}

export type UpdateProjectPayload = {
  name?: string
  status?: ProjectStatus
}

export function getProjects(filters: GetProjectsFilters = {}) {
  const searchParams = new URLSearchParams()
  if (filters.search) searchParams.set('search', filters.search)
  if (filters.status) searchParams.set('status', filters.status)
  if (filters.limit) searchParams.set('limit', String(filters.limit))
  if (filters.cursor) searchParams.set('cursor', filters.cursor)

  const query = searchParams.toString()
  return adminRequest<ProjectPage>(`/api/admin/projects${query ? `?${query}` : ''}`)
}

export function createProject(payload: CreateProjectPayload) {
  return adminRequest<Project>('/api/admin/projects', {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function updateProject(id: number, payload: UpdateProjectPayload) {
  return adminRequest<Project>(`/api/admin/projects/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(payload)
  })
}

export function deleteProject(id: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/projects/${id}`, {
    method: 'DELETE'
  })
}

export function getProjectMembers(id: number) {
  return adminRequest<ProjectMember[]>(`/api/admin/projects/${id}/members`)
}
