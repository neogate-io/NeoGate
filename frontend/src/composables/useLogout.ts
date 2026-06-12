import { useRouter } from 'vue-router'
import type { MessageKey } from '../i18n'
import { useAuthStore } from '../stores/auth'
import { confirmAction } from '../utils/confirm'

export function useLogout(t: (key: MessageKey) => string) {
  const auth = useAuthStore()
  const router = useRouter()

  return async function logout() {
    const confirmed = await confirmAction(t('logoutConfirmMessage'), t('logoutConfirmTitle'), {
      confirmText: t('logout'),
      cancelText: t('cancel'),
      type: 'warning'
    })
    if (!confirmed) return

    auth.clearToken()
    await router.replace('/')
  }
}
