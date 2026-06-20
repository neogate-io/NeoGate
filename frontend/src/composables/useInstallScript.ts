import { computed } from 'vue'
import type { MessageKey } from '../i18n'
import { copyTextWithMessage } from '../utils/clipboard'

export function useInstallScript(t: (key: MessageKey) => string) {
  const installScript = computed(() => {
    if (navigator.userAgent.includes('Windows')) {
      return `irm ${window.location.origin}/install.ps1 | iex`
    }
    return `curl -fsSL ${window.location.origin}/install | bash`
  })

  async function copyInstallScript() {
    await copyTextWithMessage(installScript.value, t('installScriptCopied'))
  }

  return {
    installScript,
    copyInstallScript
  }
}
