import { defineConfig, loadEnv } from 'vite'
import vue from '@vitejs/plugin-vue'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const frontendRoot = fileURLToPath(new URL('.', import.meta.url))
const installTemplatePath = new URL('./install.template', import.meta.url)
const defaultBackendOrigin = 'http://127.0.0.1:8080'

function requestOrigin(headers: Record<string, string | string[] | undefined>) {
  const forwarded = parseForwarded(firstHeader(headers.forwarded))
  const forwardedProto = forwarded.proto || firstHeader(headers['x-forwarded-proto'])
  const forwardedHost = forwarded.host || firstHeader(headers['x-forwarded-host'])
  const host = forwardedHost || firstHeader(headers.host) || 'localhost:5173'
  const proto =
    forwardedProto ||
    (host.startsWith('localhost') || host.startsWith('127.0.0.1') ? 'http' : 'https')
  return `${proto}://${host}`
}

function firstHeader(value: string | string[] | undefined) {
  return Array.isArray(value) ? value[0] : value
}

function parseForwarded(value: string | undefined) {
  const first = value?.split(',')[0]
  if (!first) {
    return {}
  }

  return Object.fromEntries(
    first
      .split(';')
      .map((part) => part.trim().split('='))
      .filter(([key, headerValue]) => key && headerValue)
      .map(([key, headerValue]) => [key.toLowerCase(), headerValue.replace(/^"|"$/g, '')])
  ) as { host?: string; proto?: string }
}

function normalizeBackendOrigin(value: string | undefined) {
  const origin = value?.trim()
  if (!origin) {
    return defaultBackendOrigin
  }

  return origin
    .replace(/\/+$/, '')
    .replace(/\/v1$/, '')
    .replace(/\/anthropic$/, '')
}

function renderInstallScript(installOrigin: string, backendOrigin: string) {
  return readFileSync(installTemplatePath, 'utf8')
    .replaceAll('__NEOGATE_DEFAULT_BASE_URL__', `${backendOrigin}/v1`)
    .replaceAll('__NEOGATE_INSTALL_ORIGIN__', installOrigin)
}

function isIgnorablePureAnnotationWarning(log: { code?: string; id?: string; message: string }) {
  const source = `${log.id || ''}\n${log.message}`
  return (
    log.code === 'INVALID_ANNOTATION' &&
    source.includes('@vueuse/core/dist/index.js') &&
    source.includes('#__PURE__')
  )
}

function installMiddleware(
  backendOrigin: string,
  req: { url?: string; headers: Record<string, string | string[] | undefined> },
  res: {
    statusCode: number
    setHeader(name: string, value: string): void
    end(body: string): void
  },
  next: () => void
) {
  if ((req.url || '').split('?')[0] !== '/install') {
    next()
    return
  }

  const script = renderInstallScript(requestOrigin(req.headers), backendOrigin)
  res.statusCode = 200
  res.setHeader('content-type', 'text/x-shellscript; charset=utf-8')
  res.setHeader('cache-control', 'no-store')
  res.end(script)
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, frontendRoot)
  const backendOrigin = normalizeBackendOrigin(env.VITE_NEOGATE_BACKEND_ORIGIN)

  return {
    plugins: [
      {
        name: 'neogate-install-script',
        enforce: 'pre',
        configureServer(server) {
          server.middlewares.use((req, res, next) =>
            installMiddleware(backendOrigin, req, res, next)
          )
        },
        configurePreviewServer(server) {
          server.middlewares.use((req, res, next) =>
            installMiddleware(backendOrigin, req, res, next)
          )
        }
      },
      vue()
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
        '/api': 'http://127.0.0.1:8080'
      }
    }
  }
})
