import { computed, ref } from 'vue'

export type BusinessSetupStep =
  | 'admin-password'
  | 'service-mode'
  | 'upstream'
  | 'smtp'
  | 'payment'
  | 'finish'

const optionalSteps = new Set<BusinessSetupStep>(['upstream', 'smtp', 'payment'])

export function useSetupWizard(options: {
  configureSmtp: () => boolean
  showPayment: () => boolean
  showBusinessSetup: () => boolean
}) {
  const currentBusinessStep = ref<BusinessSetupStep>('admin-password')
  const includeUpstream = ref(true)
  const includePayment = ref(true)
  const reviewingRuntimeConfig = ref(false)

  const businessSetupSteps = computed<BusinessSetupStep[]>(() => [
    'admin-password',
    'service-mode',
    'upstream',
    ...(options.configureSmtp() ? (['smtp'] as const) : []),
    ...(options.showPayment() ? (['payment'] as const) : []),
    'finish'
  ])
  const currentBusinessStepIndex = computed(() =>
    businessSetupSteps.value.indexOf(currentBusinessStep.value)
  )
  const isLastBusinessStep = computed(
    () => currentBusinessStepIndex.value === businessSetupSteps.value.length - 1
  )
  const canSkipCurrentBusinessStep = computed(() => optionalSteps.has(currentBusinessStep.value))

  function goToAdjacentBusinessStep(offset: -1 | 1) {
    const nextStep = businessSetupSteps.value[currentBusinessStepIndex.value + offset]
    if (nextStep) currentBusinessStep.value = nextStep
  }

  function isBusinessStepActive(step: BusinessSetupStep) {
    return options.showBusinessSetup() && currentBusinessStep.value === step
  }

  function isBusinessStepDone(step: BusinessSetupStep) {
    if (!options.showBusinessSetup()) return false
    return currentBusinessStepIndex.value > businessSetupSteps.value.indexOf(step)
  }

  return {
    currentBusinessStep,
    includeUpstream,
    includePayment,
    reviewingRuntimeConfig,
    businessSetupSteps,
    currentBusinessStepIndex,
    isLastBusinessStep,
    canSkipCurrentBusinessStep,
    goToAdjacentBusinessStep,
    isBusinessStepActive,
    isBusinessStepDone
  }
}
