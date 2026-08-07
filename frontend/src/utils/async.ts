export function abortableDelay(ms: number, signal?: AbortSignal) {
  if (signal?.aborted) return Promise.reject(abortError())

  return new Promise<void>((resolve, reject) => {
    const timer = globalThis.setTimeout(() => {
      cleanup()
      resolve()
    }, ms)

    const handleAbort = () => {
      globalThis.clearTimeout(timer)
      cleanup()
      reject(abortError())
    }

    const cleanup = () => signal?.removeEventListener('abort', handleAbort)
    signal?.addEventListener('abort', handleAbort, { once: true })
  })
}

export function isAbortError(error: unknown) {
  return error instanceof Error && error.name === 'AbortError'
}

function abortError() {
  const error = new Error('The operation was aborted')
  error.name = 'AbortError'
  return error
}
