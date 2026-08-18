import { describe, expect, it } from 'vitest'
import { ApiError, isSessionExpiredError } from './errors'

describe('isSessionExpiredError', () => {
  it('only treats 401 responses as an expired session', () => {
    expect(isSessionExpiredError(new ApiError('unauthorized', 401, 'unauthorized'))).toBe(true)
    expect(isSessionExpiredError(new ApiError('forbidden', 403, 'forbidden'))).toBe(false)
    expect(isSessionExpiredError(new ApiError('quota', 403, 'insufficient_quota'))).toBe(false)
  })
})
