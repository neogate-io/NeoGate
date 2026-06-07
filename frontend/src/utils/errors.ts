export function readError(err: unknown) {
  const message = err instanceof Error ? err.message : String(err)
  return message.replace(/^(bad request|conflict|payload too large|rate limited):\s*/i, '')
}

export class ApiError extends Error {
  constructor(
    message: string,
    public readonly status: number
  ) {
    super(message)
    this.name = 'ApiError'
  }
}
