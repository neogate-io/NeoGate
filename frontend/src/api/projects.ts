import type {
  CursorPage,
  Project,
  ProjectMember,
  ProjectModel,
  ProjectStatus
} from '../types/admin'
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
  owner_user_id?: number
  status?: ProjectStatus
}

export type ProjectMemberRolePayload = {
  role: 'admin' | 'member'
}

export type AddProjectMemberPayload = ProjectMemberRolePayload & {
  user_id: number
}

export type CreatedProjectMember = {
  record: ProjectMember
  key?: string | null
}

export type CreatedProject = {
  record: Project
  key?: string | null
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
  return adminRequest<CreatedProject>('/api/admin/projects', {
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

export function addProjectMember(id: number, payload: AddProjectMemberPayload) {
  return adminRequest<CreatedProjectMember>(`/api/admin/projects/${id}/members`, {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function updateProjectMember(
  projectId: number,
  memberId: number,
  payload: ProjectMemberRolePayload
) {
  return adminRequest<ProjectMember>(`/api/admin/projects/${projectId}/members/${memberId}`, {
    method: 'PATCH',
    body: JSON.stringify(payload)
  })
}

export function deleteProjectMember(projectId: number, memberId: number) {
  return adminRequest<{ ok: boolean }>(`/api/admin/projects/${projectId}/members/${memberId}`, {
    method: 'DELETE'
  })
}

export function getProjectModels(projectId: number) {
  return adminRequest<ProjectModel[]>(`/api/admin/projects/${projectId}/models`)
}

export function createProjectModel(
  projectId: number,
  payload: {
    model: string
    target_model: string
    target_channel_id?: number | null
    enabled?: boolean
    description?: string
  }
) {
  return adminRequest<ProjectModel>(`/api/admin/projects/${projectId}/models`, {
    method: 'POST',
    body: JSON.stringify(payload)
  })
}

export function updateProjectModel(
  projectId: number,
  model: string,
  payload: {
    model?: string
    target_model?: string
    target_channel_id?: number | null
    enabled?: boolean
    description?: string
  }
) {
  return adminRequest<ProjectModel>(
    `/api/admin/projects/${projectId}/models/${encodeURIComponent(model)}`,
    {
      method: 'PATCH',
      body: JSON.stringify({
        ...payload,
        target_channel_id: Object.prototype.hasOwnProperty.call(payload, 'target_channel_id')
          ? payload.target_channel_id
          : undefined
      })
    }
  )
}

export function deleteProjectModel(projectId: number, model: string) {
  return adminRequest<{ ok: boolean }>(
    `/api/admin/projects/${projectId}/models/${encodeURIComponent(model)}`,
    { method: 'DELETE' }
  )
}
