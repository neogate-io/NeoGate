import { describe, expect, it } from 'vitest'
import type { PricingTemplate } from '../types/admin'
import {
  estimatedVideoTokensPerSecond,
  findPricingTemplate,
  pricingReferenceModelAliases,
  resolvePricingReference
} from './pricing'

describe('pricingReferenceModelAliases', () => {
  it('matches dated doubao seedance model ids to reference price ids', () => {
    expect(pricingReferenceModelAliases('doubao-seedance-1-0-pro-fast-251015')).toContain(
      'doubao-seedance-1.0-pro-fast'
    )
  })

  it('normalizes Dreamina Seedance ids to canonical Seedance references', () => {
    expect(pricingReferenceModelAliases('dreamina-seedance-2-0-260128')).toContain(
      'doubao-seedance-2.0'
    )
  })

  it('ignores billing display prefixes on Dreamina Seedance ids', () => {
    expect(pricingReferenceModelAliases('【按秒计费】dreamina-seedance-2-0-260128')).toContain(
      'doubao-seedance-2.0'
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

  it('resolves reference pricing from the base model instead of the upstream model', () => {
    const template: PricingTemplate = {
      id: 2,
      provider: 'doubao',
      model: 'doubao-seedance-2.0',
      input_price_micros: 1_000_000,
      output_price_micros: 2_000_000,
      billing_meter: 'token',
      pricing_basis: 'token',
      source: 'local',
      enabled: true,
      created_at: '',
      updated_at: ''
    }

    expect(
      resolvePricingReference([template], 'openai', 'sd_2.0_discount', 'doubao-seedance-2.0')
    ).toEqual({ provider: 'doubao', model: 'doubao-seedance-2.0' })
  })
})

describe('estimatedVideoTokensPerSecond', () => {
  it('defaults missing video resolution to a conservative 1080p estimate', () => {
    expect(estimatedVideoTokensPerSecond('*')).toBe(48600)
    expect(estimatedVideoTokensPerSecond('')).toBe(48600)
  })
})
