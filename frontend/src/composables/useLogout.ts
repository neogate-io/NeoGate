import { useRouter } from 'vue-router'
import type { MessageKey } from '../i18n'
import { useAuthStore } from '../stores/auth'
import { createConfirmAction } from '../utils/confirm'

export function useLogout(t: (key: MessageKey) => string) {
  const auth = useAuthStore()
  const router = useRouter()
  const confirmDialog = createConfirmAction(() => t('cancel'))

  return async function logout() {
    const confirmed = await confirmDialog(t('logoutConfirmMessage'), t('logoutConfirmTitle'), {
      confirmText: t('logout'),
      type: 'warning'
    })
    if (!confirmed) return

    auth.clearToken()
    await router.replace('/')
  }
}
