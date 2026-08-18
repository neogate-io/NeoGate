import type { BillingMeter, VideoBillingMode } from './channel'

export type ChannelVideoPriceTierForm = {
  resolutionsText: string
  resolutionLabel?: string
  pricePairLeftLabel?: string
  pricePairRightLabel?: string
  inputWithVideo: number
  inputWithoutVideo: number
  estimatedTokensPerSecond: number
  inputWithVideoUnit: number
  inputWithoutVideoUnit: number
  singlePrice?: boolean
}

export type ChannelPriceForm = {
  channelId: number
  provider: string
  model: string
  referenceProvider: string
  referenceModel: string
  modelCategory: 'text' | 'image' | 'video' | 'audio'
  audioTranscriptionMode: 'file' | 'realtime' | null
  billingMeter: BillingMeter | null
  videoBillingMode: VideoBillingMode | null
  videoPriceTiers: ChannelVideoPriceTierForm[]
  inputPerMillion: number
  outputPerMillion: number
  cacheReadPerMillion: number
  cacheWritePerMillion: number | null
  unitPrice: number
  enabled: boolean
  hasPrice: boolean
  hasPriceRecord: boolean
  billingMeterLocked: boolean
  videoBillingModeLocked: boolean
  canUseImageBilling: boolean
  canUseVideoBilling: boolean
}
