import { userRequest } from './request'

export type UserOverview = {
  email: string
  display_name: string
  balance_micro_usd: number
  reserved_micro_usd: number
  available_micro_usd: number
  today_cost_micro_usd: number
  month_cost_micro_usd: number
  request_count: number
  daily_costs: DailyCost[]
}

export type DailyCost = {
  date: string
  cost_micro_usd: number
}

export function getUserOverview() {
  return userRequest<UserOverview>('/api/user/overview')
}
