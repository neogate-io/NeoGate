import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { getCurrentUser } from '../api/me'
import type { LoginRole } from '../api/auth'

const tokenStorageKey = 'neogate_token'
const roleStorageKey = 'neogate_role'

export const useAuthStore = defineStore('auth', () => {
  const token = ref(localStorage.getItem(tokenStorageKey) || '')
  const storedRole = localStorage.getItem(roleStorageKey)
  const role = ref<LoginRole | ''>(
    (isLoginRole(storedRole) ? storedRole : null) || roleFromToken(token.value)
  )
  const sessionChecked = ref(token.value.length === 0)
  const verifiedToken = ref('')
  const isAuthed = computed(() => token.value.length > 0)
  const isAdmin = computed(() => role.value === 'admin')
  const isUser = computed(() => role.value === 'user')
  let sessionPromise: Promise<boolean> | null = null

  function setToken(nextToken: string, nextRole: LoginRole) {
    token.value = nextToken
    role.value = nextRole
    verifiedToken.value = nextToken
    sessionChecked.value = true
  }

  function clearToken() {
    token.value = ''
    role.value = ''
    verifiedToken.value = ''
    sessionChecked.value = true
    sessionPromise = null
  }

  async function verifySession(force = false) {
    if (!token.value) {
      clearToken()
      return false
    }

    if (!force && sessionChecked.value && verifiedToken.value === token.value) {
      return isAuthed.value && isLoginRole(role.value)
    }

    if (sessionPromise) return sessionPromise

    const requestToken = token.value
    sessionPromise = getCurrentUser(requestToken)
      .then((currentUser) => {
        if (token.value !== requestToken) {
          return Boolean(token.value)
        }

        role.value = currentUser.role
        verifiedToken.value = requestToken
        sessionChecked.value = true
        return true
      })
      .catch(() => {
        if (token.value === requestToken) {
          clearToken()
        }
        return false
      })
      .finally(() => {
        sessionPromise = null
      })

    return sessionPromise
  }

  watch(
    token,
    (nextToken) => {
      if (nextToken) {
        localStorage.setItem(tokenStorageKey, nextToken)
      } else {
        localStorage.removeItem(tokenStorageKey)
      }
      if (nextToken !== verifiedToken.value) {
        sessionChecked.value = nextToken.length === 0
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
    sessionChecked,
    setToken,
    clearToken,
    verifySession
  }
})

function roleFromToken(token: string): LoginRole | '' {
  if (token.startsWith('neo_admin_v1_')) return 'admin'
  if (token.startsWith('neo_user_v1_')) return 'user'
  return ''
}

function isLoginRole(value: unknown): value is LoginRole {
  return value === 'admin' || value === 'user'
}
