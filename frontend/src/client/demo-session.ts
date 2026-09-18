import { useSyncExternalStore } from 'react'
import { acceptOrder, cancelOrder, initialState, moveFunds, settleOrder } from '../domain/demo'
import type { DemoState, OrderInput, Scenario } from '../domain/demo'

// Browser-only simulation: no wallet provider, fetch, storage, or canister calls.
let state = initialState()
const emptyServerState = initialState()
const listeners = new Set<() => void>()
let generation = 0
let pending = new Map<string, ReturnType<typeof setTimeout>>()
function publish(next: DemoState) {
  state = next
  listeners.forEach((listener) => listener())
}
function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
export function useDemo() {
  return useSyncExternalStore(
    subscribe,
    () => state,
    () => emptyServerState,
  )
}
export function startDemo() {
  if (typeof window === 'undefined') throw new Error('Browser only')
  generation++
  publish({
    ...initialState(),
    active: true,
    observedAt: Date.now(),
    reserve: 5_000_000_000,
    trading: 10_000_000_000,
  })
}
export function logoutDemo() {
  generation++
  pending.forEach(clearTimeout)
  pending = new Map()
  publish(initialState())
}
export function setScenario(scenario: Scenario) {
  publish({ ...state, scenario })
}
export function setStale(stale: boolean) {
  publish({ ...state, observedAt: Date.now() - (stale ? 60_000 : 0) })
}
export function refreshDemo() {
  if (state.active) publish({ ...state, observedAt: Date.now() })
}
export function submitDemo(input: OrderInput, id: string) {
  const next = acceptOrder(state, input, id, Date.now())
  if (next === state) return
  publish(next)
  const epoch = generation
  const scenario = state.scenario
  pending.set(
    id,
    setTimeout(() => {
      pending.delete(id)
      if (epoch === generation) publish(settleOrder(state, id, scenario))
    }, 700),
  )
}
export function cancelDemo(id: string) {
  publish(cancelOrder(state, id, state.scenario === 'cancel-race'))
}
export function transferDemo(
  id: string,
  kind: 'deposit' | 'allocate' | 'recover' | 'withdraw',
  amount: number,
) {
  publish(moveFunds(state, id, kind, amount, new Date().toLocaleTimeString('ja-JP')))
}
