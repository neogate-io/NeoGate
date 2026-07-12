import { describe, expect, it } from 'vitest'
import type { PricingTemplate } from '../types/admin'
import {
  estimatedVideoTokensPerSecond,
  findPricingTemplate,
  pricingReferenceModelAliases
} from './pricing'

describe('pricingReferenceModelAliases', () => {
  it('matches dated doubao seedance model ids to reference price ids', () => {
    expect(pricingReferenceModelAliases('doubao-seedance-1-0-pro-fast-251015')).toContain(
      'doubao-seedance-1.0-pro-fast'
    )
  })

  it('finds token-priced reference templates for dated doubao seedance model ids', () => {
    const template: PricingTemplate = {
      id: 1,
      provider: 'doubao',
      model: 'doubao-seedance-1.0-pro-fast',
      input_price_micros: 4_200_000,
      output_price_micros: 4_200_000,
      billing_meter: 'token',
      pricing_basis: 'token',
      source: 'local',
      enabled: true,
      created_at: '',
      updated_at: ''
    }

    expect(findPricingTemplate([template], 'doubao', 'doubao-seedance-1-0-pro-fast-251015')).toBe(
      template
    )
  })
})

describe('estimatedVideoTokensPerSecond', () => {
  it('defaults missing video resolution to the backend 480p assumption', () => {
    expect(estimatedVideoTokensPerSecond('*')).toBe(10080)
    expect(estimatedVideoTokensPerSecond('')).toBe(10080)
  })
})
