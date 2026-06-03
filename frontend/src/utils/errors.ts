export function readError(err: unknown) {
  return err instanceof Error ? err.message : String(err)
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
