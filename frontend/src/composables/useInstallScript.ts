import { computed } from 'vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import type { MessageKey } from '../i18n'
import { copyTextToClipboard } from '../utils/clipboard'

export function useInstallScript(t: (key: MessageKey) => string) {
  const installScript = computed(() => {
    return `curl -fsSL ${window.location.origin}/install | bash`
  })

  async function copyInstallScript() {
    await copyTextToClipboard(installScript.value)
    ElMessage.success(t('installScriptCopied'))
  }

  return {
    installScript,
    copyInstallScript
  }
}
