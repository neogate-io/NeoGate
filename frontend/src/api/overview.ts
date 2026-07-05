import { userRequest } from './request'

export type UserOverview = {
  email: string
  display_name: string
  balance_micros: number
  reserved_micros: number
  available_micros: number
  today_cost_micros: number
  month_cost_micros: number
  request_count: number
  daily_costs: DailyCost[]
}

export type DailyCost = {
  date: string
  cost_micros: number
}

export function getUserOverview() {
  return userRequest<UserOverview>('/api/user/overview')
}
