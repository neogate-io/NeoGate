export function readError(err: unknown) {
  const message = err instanceof Error ? err.message : String(err)
  return message.replace(/^(bad request|conflict|payload too large|rate limited):\s*/i, '')
}

export function isSmtpConfigError(err: unknown) {
  if (!(err instanceof ApiError)) return false
  const message = err.message.toLowerCase()
  return (
    message.includes('smtp settings are not configured') ||
    message.includes('smtp settings are invalid') ||
    message.includes('smtp host is not configured') ||
    message.includes('smtp port is invalid') ||
    message.includes('smtp sender email is not configured') ||
    message.includes('smtp sender email is invalid')
  )
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
