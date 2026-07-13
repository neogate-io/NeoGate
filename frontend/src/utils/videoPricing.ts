import type { VideoBillingMode, VideoPriceTier } from '../types/admin'
import { splitCommaList } from './channel'

export const ANY_VIDEO_TIER_RESOLUTION = '*'

export type ReferenceVideoTier = {
  resolution?: string
  label?: string
  pricePairKind?: 'input_output'
  tiers?: Record<string, number | null | undefined>
}

export type VideoPriceShape = 'single' | 'input_video_pair' | 'audio_pair'

export function referenceVideoTierResolutions(tier: ReferenceVideoTier) {
  const resolution = tier.resolution?.trim()
  return resolution ? splitCommaList(resolution) : [ANY_VIDEO_TIER_RESOLUTION]
}

export function referenceVideoTierHasAudio(tier: ReferenceVideoTier) {
  return tier.tiers?.with_audio != null || tier.tiers?.without_audio != null
}

export function referenceVideoTierUsesSinglePrice(tier: ReferenceVideoTier) {
  return (
    tier.tiers?.price != null &&
    tier.tiers?.input_without_video == null &&
    tier.tiers?.input_with_video == null &&
    tier.tiers?.without_audio == null &&
    tier.tiers?.with_audio == null
  )
}

export function referenceVideoTierUsesInputOutputLabels(tier: ReferenceVideoTier) {
  return tier.pricePairKind === 'input_output'
}

export function referenceVideoTierPrice(
  tier: ReferenceVideoTier,
  kind: 'input_without_video' | 'input_with_video'
) {
  if (kind === 'input_with_video') {
    return tier.tiers?.input_with_video ?? tier.tiers?.with_audio ?? tier.tiers?.price ?? 0
  }
  return tier.tiers?.input_without_video ?? tier.tiers?.without_audio ?? tier.tiers?.price ?? 0
}

export function referenceVideoTierInputWithoutVideo(tier: ReferenceVideoTier) {
  return referenceVideoTierPrice(tier, 'input_without_video')
}

export function referenceVideoTierInputWithVideo(tier: ReferenceVideoTier) {
  return referenceVideoTierPrice(tier, 'input_with_video')
}

export function referenceVideoTierUsesSingleTokenPrice(tier: ReferenceVideoTier) {
  return (
    referenceVideoTierUsesInputOutputLabels(tier) &&
    referenceVideoTierInputWithoutVideo(tier) === referenceVideoTierInputWithVideo(tier)
  )
}

export function referenceVideoPriceShape(tier: ReferenceVideoTier): VideoPriceShape {
  if (referenceVideoTierUsesSinglePrice(tier) || referenceVideoTierUsesSingleTokenPrice(tier)) {
    return 'single'
  }
  return referenceVideoTierHasAudio(tier) ? 'audio_pair' : 'input_video_pair'
}

export function referenceVideoTierMatchesFormResolutions(
  tier: ReferenceVideoTier,
  formResolutions: Set<string>
) {
  return referenceVideoTierResolutions(tier).some((resolution) => {
    const normalizedResolution = resolution.trim().toLowerCase()
    return (
      normalizedResolution === ANY_VIDEO_TIER_RESOLUTION ||
      formResolutions.has(ANY_VIDEO_TIER_RESOLUTION) ||
      formResolutions.has(normalizedResolution)
    )
  })
}

export function isAnyReferenceVideoTier(tier: ReferenceVideoTier) {
  return referenceVideoTierResolutions(tier).some(
    (resolution) => resolution.trim().toLowerCase() === ANY_VIDEO_TIER_RESOLUTION
  )
}

export function savedVideoTierMatchesReferenceTier(
  tier: VideoPriceTier,
  referenceTier: ReferenceVideoTier
) {
  const formResolutions = new Set(
    tier.resolutions.flatMap(splitCommaList).map((item) => item.toLowerCase())
  )
  return referenceVideoTierMatchesFormResolutions(referenceTier, formResolutions)
}

export function lockedVideoBillingModeForReferenceTiers(
  tiers: ReferenceVideoTier[]
): VideoBillingMode | null {
  if (tiers.length === 0) return null
  return tiers.every(referenceVideoTierUsesSinglePrice) ? 'per_second' : null
}

export function defaultVideoBillingModeForReferenceTiers(
  tiers: ReferenceVideoTier[]
): VideoBillingMode {
  return lockedVideoBillingModeForReferenceTiers(tiers) ?? 'official_token'
}
