import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

const localBackendOrigin = 'http://127.0.0.1:8080'
const apiProxy = {
  target: localBackendOrigin,
  timeout: 10 * 60 * 1000,
  proxyTimeout: 10 * 60 * 1000
}

function isIgnorablePureAnnotationWarning(log: { code?: string; id?: string; message: string }) {
  const source = `${log.id || ''}\n${log.message}`
  return (
    log.code === 'INVALID_ANNOTATION' &&
    source.includes('@vueuse/core/dist/index.js') &&
    source.includes('#__PURE__')
  )
}

export default defineConfig({
  plugins: [vue()],
  build: {
    rollupOptions: {
      onLog(level, log, defaultHandler) {
        if (isIgnorablePureAnnotationWarning(log)) {
          return
        }
        defaultHandler(level, log)
      },
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/@element-plus/icons-vue')) {
            return 'element-icons'
          }
          if (id.includes('node_modules/element-plus')) {
            if (id.includes('/components/table/')) return 'element-table'
            if (
              id.includes('/components/dialog/') ||
              id.includes('/components/message/') ||
              id.includes('/components/message-box/') ||
              id.includes('/components/loading/') ||
              id.includes('/components/tooltip/')
            ) {
              return 'element-feedback'
            }
            if (
              id.includes('/components/form/') ||
              id.includes('/components/input/') ||
              id.includes('/components/input-number/') ||
              id.includes('/components/select/') ||
              id.includes('/components/date-picker/') ||
              id.includes('/components/switch/')
            ) {
              return 'element-form'
            }
            if (
              id.includes('/components/button/') ||
              id.includes('/components/menu/') ||
              id.includes('/components/dropdown/') ||
              id.includes('/components/pagination/') ||
              id.includes('/components/segmented/')
            ) {
              return 'element-controls'
            }
            return 'element-core'
          }
          if (
            id.includes('node_modules/vue') ||
            id.includes('node_modules/vue-router') ||
            id.includes('node_modules/pinia')
          ) {
            return 'vue'
          }
        }
      }
    }
  },
  server: {
    port: 5173,
    proxy: {
      '/api': apiProxy,
      '/v1': apiProxy,
      '/anthropic': apiProxy,
      '/install': apiProxy,
      '/install.ps1': apiProxy
    }
  }
})
