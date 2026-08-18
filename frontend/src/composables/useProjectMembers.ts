import { computed, reactive, ref, type Ref } from 'vue'
import { ElMessage } from 'element-plus'
import { addProjectMember, deleteProjectMember, getProjectMembers } from '../api/projects'
import { getUsers } from '../api/users'
import type { MessageKey } from '../i18n'
import type { Project, ProjectMember, User } from '../types/admin'
import { copyTextWithMessage } from '../utils/clipboard'
import { createConfirmAction } from '../utils/confirm'
import { readError } from '../utils/errors'
import { useLatestTask } from './useLatestTask'
import { withLoading, withLoadingValue } from './useLoadingTask'

type Translate = (key: MessageKey) => string
type EditableProjectMemberRole = Extract<ProjectMember['role'], 'admin' | 'member'>

export function useProjectMembers(
  t: Translate,
  selectedProject: Ref<Project | null>,
  reloadProjects: () => Promise<void>
) {
  const confirmDialog = createConfirmAction(() => t('cancel'))
  const membersDialogVisible = ref(false)
  const membersLoading = ref(false)
  const memberSaving = ref(false)
  const memberUserOptions = ref<User[]>([])
  const memberUserSearchLoading = ref(false)
  const deletingMemberId = ref<number | null>(null)
  const selectedMembers = ref<ProjectMember[]>([])
  const memberForm = reactive<{ userId: number | null; role: EditableProjectMemberRole }>({
    userId: null,
    role: 'member'
  })
  const membersTask = useLatestTask(membersLoading)
  const memberSearchTask = useLatestTask(memberUserSearchLoading)

  const editableMemberRoleOptions = computed(() => [
    { label: t('memberRoleAdmin'), value: 'admin' as const },
    { label: t('memberRoleMember'), value: 'member' as const }
  ])

  async function openMembersDialog(row: Project) {
    selectedProject.value = row
    membersDialogVisible.value = true
    selectedMembers.value = []
    memberUserOptions.value = []
    Object.assign(memberForm, { userId: null, role: 'member' })
    try {
      await loadSelectedProjectMembers()
    } catch (err) {
      ElMessage.error(readError(err))
    }
  }

  async function loadSelectedProjectMembers() {
    const projectId = selectedProject.value?.id
    if (!projectId) return
    await membersTask.run(
      () => getProjectMembers(projectId),
      (members) => {
        if (selectedProject.value?.id === projectId) selectedMembers.value = members
      }
    )
  }

  async function searchMemberUsers(query: string) {
    const search = query.trim()
    if (!search) {
      memberSearchTask.invalidate()
      memberUserOptions.value = []
      return
    }
    try {
      await memberSearchTask.run(
        () => getUsers({ search, limit: 20 }),
        (page) => {
          memberUserOptions.value = page.items
        }
      )
    } catch (err) {
      ElMessage.error(readError(err))
    }
  }

  async function submitAddProjectMember() {
    const projectId = selectedProject.value?.id
    if (!projectId) return
    if (!memberForm.userId) {
      ElMessage.error(t('projectMemberRequired'))
      return
    }
    await withLoading(memberSaving, async () => {
      try {
        await addProjectMember(projectId, {
          user_id: memberForm.userId as number,
          role: memberForm.role
        })
        ElMessage.success(t('projectMemberAdded'))
        Object.assign(memberForm, { userId: null, role: 'member' })
        memberUserOptions.value = []
        await loadSelectedProjectMembers()
        await reloadProjects()
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
  }

  async function confirmDeleteProjectMember(row: ProjectMember) {
    if (!selectedProject.value || row.role === 'owner') return
    const confirmed = await confirmDialog(
      t('deleteProjectMemberConfirm').replace('{email}', row.user_email),
      t('confirmDelete'),
      { confirmText: t('delete'), danger: true, type: 'warning' }
    )
    if (!confirmed) return
    const projectId = selectedProject.value.id
    await withLoadingValue(deletingMemberId, row.id, null, async () => {
      try {
        await deleteProjectMember(projectId, row.id)
        ElMessage.success(t('projectMemberRemoved'))
        await loadSelectedProjectMembers()
        await reloadProjects()
      } catch (err) {
        ElMessage.error(readError(err))
      }
    })
  }

  function projectMemberDisplayName(member: ProjectMember) {
    const name = member.user_username || member.user_email
    return member.role === 'admin' ? `${name}（${t('memberRoleAdmin')}）` : name
  }

  function copyApiKeyValue(value: string) {
    return copyTextWithMessage(value, t('apiKeyCopied'))
  }

  return {
    membersDialogVisible,
    membersLoading,
    memberSaving,
    memberUserOptions,
    memberUserSearchLoading,
    deletingMemberId,
    selectedMembers,
    memberForm,
    editableMemberRoleOptions,
    openMembersDialog,
    searchMemberUsers,
    submitAddProjectMember,
    confirmDeleteProjectMember,
    projectMemberDisplayName,
    copyApiKeyValue
  }
}
