import type { PaymentOrder } from '../api/recharge'
import { abortableDelay } from './async'

export type PaymentReturnState = 'paid' | 'pending' | 'failed' | 'unknown'

export function findPaymentOrder(orders: PaymentOrder[], orderNo: string) {
  if (!orderNo) return null
  return orders.find((order) => String(order.order_no) === orderNo) ?? null
}

export function paymentReturnState(order: PaymentOrder | null): PaymentReturnState {
  if (!order) return 'unknown'
  if (order.status === 'paid') return 'paid'
  if (order.status === 'pending') return 'pending'
  return 'failed'
}

export async function pollPaymentOrder(
  orderNo: string,
  loadOrders: (signal: AbortSignal) => Promise<PaymentOrder[]>,
  options: {
    attempts: number
    intervalMs: number
    signal: AbortSignal
    wait?: (ms: number, signal: AbortSignal) => Promise<void>
  }
) {
  const wait = options.wait ?? abortableDelay
  let order: PaymentOrder | null = null

  for (let attempt = 0; attempt < options.attempts; attempt += 1) {
    order = findPaymentOrder(await loadOrders(options.signal), orderNo)
    if (order && order.status !== 'pending') return order
    if (attempt < options.attempts - 1) {
      await wait(options.intervalMs, options.signal)
    }
  }

  return order
}
