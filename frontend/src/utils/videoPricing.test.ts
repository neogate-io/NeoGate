import { describe, expect, it } from 'vitest'
import type { VideoPriceTier } from '../types/admin'
import {
  ANY_VIDEO_TIER_RESOLUTION,
  defaultVideoBillingModeForReferenceTiers,
  referenceVideoPriceShape,
  referenceVideoTierMatchesFormResolutions,
  referenceVideoTierUsesSingleTokenPrice,
  savedVideoTierMatchesReferenceTier,
  type ReferenceVideoTier
} from './videoPricing'

describe('referenceVideoPriceShape', () => {
  it('treats per-resolution price tiers as single price video tiers', () => {
    const tier: ReferenceVideoTier = {
      resolution: '720P',
      tiers: { price: 0.9 }
    }

    expect(referenceVideoPriceShape(tier)).toBe('single')
    expect(defaultVideoBillingModeForReferenceTiers([tier])).toBe('per_second')
  })

  it('treats equal input/output token references as single token prices', () => {
    const tier: ReferenceVideoTier = {
      pricePairKind: 'input_output',
      tiers: {
        input_without_video: 4.2,
        input_with_video: 4.2
      }
    }

    expect(referenceVideoTierUsesSingleTokenPrice(tier)).toBe(true)
    expect(referenceVideoPriceShape(tier)).toBe('single')
    expect(defaultVideoBillingModeForReferenceTiers([tier])).toBe('official_token')
  })

  it('keeps input-with-video tiers as paired prices', () => {
    const tier: ReferenceVideoTier = {
      resolution: '480p,720p',
      tiers: {
        input_without_video: 37,
        input_with_video: 22
      }
    }

    expect(referenceVideoPriceShape(tier)).toBe('input_video_pair')
  })
})

describe('referenceVideoTierMatchesFormResolutions', () => {
  it('matches wildcard references and saved resolution tiers', () => {
    const wildcard: ReferenceVideoTier = { tiers: { price: 1 } }
    const saved: VideoPriceTier = {
      resolutions: [ANY_VIDEO_TIER_RESOLUTION],
      input_without_video_unit_micros: 1_000_000
    }

    expect(
      referenceVideoTierMatchesFormResolutions(wildcard, new Set([ANY_VIDEO_TIER_RESOLUTION]))
    ).toBe(true)
    expect(savedVideoTierMatchesReferenceTier(saved, wildcard)).toBe(true)
  })
})
