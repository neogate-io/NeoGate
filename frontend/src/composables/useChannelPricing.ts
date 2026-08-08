import { computed, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import {
  getModelReferenceCatalog,
  getProviderModels,
  getPricingTemplates,
  getChannelPrices,
  syncPricingTemplates
} from '../api/prices'
import type {
  ChannelPrice,
  ModelReferenceCatalogRecord,
  PricingTemplate,
  ProviderModel
} from '../types/admin'
import type { ChannelPriceForm } from '../types/channelPricing'
import type { MessageKey } from '../i18n'
import { ApiError, readError } from '../utils/errors'
import { channelPriceKey, priceKey } from '../utils/pricing'
import { withLoading } from './useLoadingTask'

type Translate = (key: MessageKey) => string

export function useChannelPricing(t: Translate) {
  const prices = ref<ChannelPrice[]>([])
  const templates = ref<PricingTemplate[]>([])
  const providerModels = ref<ProviderModel[]>([])
  const modelReferenceCatalog = ref<ModelReferenceCatalogRecord[]>([])
  const pricingLoading = ref(true)
  const priceDialogOpen = ref(false)
  const savingPrices = ref(false)
  const priceForms = reactive<Record<string, ChannelPriceForm>>({})

  const priceByModel = computed(
    () =>
      new Map(prices.value.map((price) => [channelPriceKey(price.channel_id, price.model), price]))
  )
  const providerModelByModel = computed(
    () =>
      new Map(providerModels.value.map((model) => [priceKey(model.provider, model.model), model]))
  )

  async function loadPricingData() {
    await withLoading(pricingLoading, async () => {
      try {
        const [fetchedPrices, fetchedTemplates, fetchedProviderModels, fetchedCatalog] =
          await Promise.all([
            getChannelPrices(),
            getPricingTemplates(),
            getProviderModels(),
            getModelReferenceCatalog()
          ])
        prices.value = fetchedPrices
        templates.value = fetchedTemplates
        providerModels.value = fetchedProviderModels
        modelReferenceCatalog.value = fetchedCatalog
      } catch (err) {
        prices.value = []
        templates.value = []
        providerModels.value = []
        modelReferenceCatalog.value = []
        ElMessage.error(readError(err))
      }
    })
  }

  async function syncReferencePricesIfNeeded(needed: boolean) {
    if (!needed) return true
    try {
      await syncPricingTemplates()
      const [fetchedTemplates, fetchedProviderModels, fetchedCatalog] = await Promise.all([
        getPricingTemplates(),
        getProviderModels(),
        getModelReferenceCatalog()
      ])
      templates.value = fetchedTemplates
      providerModels.value = fetchedProviderModels
      modelReferenceCatalog.value = fetchedCatalog
      return true
    } catch (err) {
      ElMessage.error(
        err instanceof ApiError && err.code === 'pricing_reference_source_unavailable'
          ? t('referencePricesSourceUnavailable')
          : readError(err)
      )
      return false
    }
  }

  function clearForms() {
    for (const key of Object.keys(priceForms)) delete priceForms[key]
  }

  return {
    prices,
    templates,
    providerModels,
    modelReferenceCatalog,
    pricingLoading,
    priceDialogOpen,
    savingPrices,
    priceForms,
    priceByModel,
    providerModelByModel,
    loadPricingData,
    syncReferencePricesIfNeeded,
    clearForms
  }
}
