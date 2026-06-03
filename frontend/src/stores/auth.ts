import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import type { LoginRole } from '../api/auth'

const tokenStorageKey = 'neogate_token'
const roleStorageKey = 'neogate_role'

export const useAuthStore = defineStore('auth', () => {
  const token = ref(localStorage.getItem(tokenStorageKey) || '')
  const role = ref<LoginRole | ''>(
    (localStorage.getItem(roleStorageKey) as LoginRole | null) || roleFromToken(token.value)
  )
  const isAuthed = computed(() => token.value.length > 0)
  const isAdmin = computed(() => role.value === 'admin')
  const isUser = computed(() => role.value === 'user')

  function setToken(nextToken: string, nextRole: LoginRole) {
    token.value = nextToken
    role.value = nextRole
  }

  function clearToken() {
    token.value = ''
    role.value = ''
  }

  watch(
    token,
    (nextToken) => {
      if (nextToken) {
        localStorage.setItem(tokenStorageKey, nextToken)
      } else {
        localStorage.removeItem(tokenStorageKey)
      }
    },
    { immediate: true }
  )

  watch(
    role,
    (nextRole) => {
      if (nextRole) {
        localStorage.setItem(roleStorageKey, nextRole)
      } else {
        localStorage.removeItem(roleStorageKey)
      }
    },
    { immediate: true }
  )

  return {
    token,
    role,
    isAuthed,
    isAdmin,
    isUser,
    setToken,
    clearToken
  }
})

function roleFromToken(token: string): LoginRole | '' {
  if (token.startsWith('neo_admin_v1_')) return 'admin'
  if (token.startsWith('neo_user_v1_')) return 'user'
  return ''
}
