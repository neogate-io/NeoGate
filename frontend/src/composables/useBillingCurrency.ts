import { computed } from 'vue'
import { useSiteBrand } from './useSiteBrand'
import {
  formatMoney as formatMoneyBase,
  formatPricePerMillion as formatPricePerMillionBase,
  majorToMicroAmount,
  microAmountToMajor,
  type BillingCurrency
} from '../utils/format'

export type { BillingCurrency }

export { majorToMicroAmount, microAmountToMajor, MICRO_UNITS_PER_CURRENCY } from '../utils/format'

export function currencySymbol(currency: BillingCurrency) {
  return currency === 'CNY' ? '¥' : '$'
}

export function useBillingCurrency() {
  const { billingCurrency } = useSiteBrand()

  return {
    billingCurrency,
    billingCurrencyCode: computed(() => billingCurrency.value),
    currencySymbol: computed(() => currencySymbol(billingCurrency.value)),
    formatMoney: (value: number | null | undefined, locale: string, digits = 2) =>
      formatMoneyBase(value, billingCurrency.value, locale, digits),
    formatPricePerMillion: (
      value: number | null | undefined,
      locale: string,
      maximumFractionDigits = 6
    ) => formatPricePerMillionBase(value, billingCurrency.value, locale, maximumFractionDigits),
    majorToMicroAmount,
    microAmountToMajor
  }
}
