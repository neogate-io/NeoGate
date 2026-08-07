import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'

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
  plugins: [
    vue(),
    Components({
      dts: false,
      directives: true,
      resolvers: [ElementPlusResolver({ importStyle: 'css' })]
    })
  ],
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
