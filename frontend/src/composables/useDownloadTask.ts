import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { readError } from '../utils/errors'

export function useDownloadTask() {
  const downloading = ref(false)

  async function run(task: () => Promise<void>) {
    if (downloading.value) return false
    downloading.value = true
    try {
      await task()
      return true
    } catch (error) {
      ElMessage.error(readError(error))
      return false
    } finally {
      downloading.value = false
    }
  }

  return { downloading, run }
}
