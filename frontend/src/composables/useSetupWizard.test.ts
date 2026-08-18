import { ref } from 'vue'
import { describe, expect, it } from 'vitest'
import { useSetupWizard } from './useSetupWizard'

describe('useSetupWizard', () => {
  it('derives optional steps and preserves the current step while navigating', () => {
    const configureSmtp = ref(false)
    const showPayment = ref(false)
    const showBusinessSetup = ref(true)
    const wizard = useSetupWizard({
      configureSmtp: () => configureSmtp.value,
      showPayment: () => showPayment.value,
      showBusinessSetup: () => showBusinessSetup.value
    })

    expect(wizard.businessSetupSteps.value).toEqual([
      'admin-password',
      'service-mode',
      'upstream',
      'finish'
    ])
    wizard.goToAdjacentBusinessStep(1)
    expect(wizard.currentBusinessStep.value).toBe('service-mode')

    configureSmtp.value = true
    showPayment.value = true
    expect(wizard.businessSetupSteps.value).toEqual([
      'admin-password',
      'service-mode',
      'upstream',
      'smtp',
      'payment',
      'finish'
    ])

    wizard.currentBusinessStep.value = 'upstream'
    expect(wizard.canSkipCurrentBusinessStep.value).toBe(true)
    wizard.goToAdjacentBusinessStep(1)
    expect(wizard.currentBusinessStep.value).toBe('smtp')
    expect(wizard.isBusinessStepDone('service-mode')).toBe(true)
  })
})
