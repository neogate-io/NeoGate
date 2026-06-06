import { ElMessageBox } from 'element-plus'
import { useRouter } from 'vue-router'
import type { MessageKey } from '../i18n'
import { useAuthStore } from '../stores/auth'

export function useLogout(t: (key: MessageKey) => string) {
  const auth = useAuthStore()
  const router = useRouter()

  return async function logout() {
    try {
      await ElMessageBox.confirm(t('logoutConfirmMessage'), t('logoutConfirmTitle'), {
        confirmButtonText: t('logout'),
        cancelButtonText: t('cancel'),
        type: 'warning'
      })
    } catch {
      return
    }

    auth.clearToken()
    await router.replace('/')
  }
}
