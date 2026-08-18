import { describe, expect, it } from 'vitest'
import type { PaymentOrder } from '../api/recharge'
import { findPaymentOrder, paymentReturnState, pollPaymentOrder } from './payment'

function paymentOrder(status: PaymentOrder['status'], orderNo = 1001): PaymentOrder {
  return {
    id: 'payment-id',
    order_no: orderNo,
    user_id: 1,
    provider: 'zpay',
    status,
    currency: 'CNY',
    amount_micros: 10_000_000,
    payable_amount_minor: 1000,
    created_at: '2026-08-08T00:00:00Z',
    updated_at: '2026-08-08T00:00:00Z'
  }
}

describe('payment return state', () => {
  it('finds only the current user order with the returned order number', () => {
    const orders = [paymentOrder('paid', 1001), paymentOrder('pending', 1002)]

    expect(findPaymentOrder(orders, '1002')).toBe(orders[1])
    expect(findPaymentOrder(orders, '9999')).toBeNull()
  })

  it('does not treat pending or failed orders as paid', () => {
    expect(paymentReturnState(paymentOrder('paid'))).toBe('paid')
    expect(paymentReturnState(paymentOrder('pending'))).toBe('pending')
    expect(paymentReturnState(paymentOrder('failed'))).toBe('failed')
    expect(paymentReturnState(null)).toBe('unknown')
  })

  it('keeps checking when an order is not visible immediately', async () => {
    const responses = [[], [paymentOrder('pending')], [paymentOrder('paid')]]
    const controller = new AbortController()
    let calls = 0

    const order = await pollPaymentOrder('1001', async () => responses[calls++] ?? [], {
      attempts: 3,
      intervalMs: 0,
      signal: controller.signal,
      wait: async () => undefined
    })

    expect(calls).toBe(3)
    expect(order?.status).toBe('paid')
  })

  it('returns the last pending state after the retry budget is exhausted', async () => {
    const controller = new AbortController()

    const order = await pollPaymentOrder('1001', async () => [paymentOrder('pending')], {
      attempts: 2,
      intervalMs: 0,
      signal: controller.signal,
      wait: async () => undefined
    })

    expect(order?.status).toBe('pending')
  })
})
