import { useSyncExternalStore } from 'react'
import {
  acceptOrder,
  cancelOrder,
  initialState,
  moveFunds,
  settleOrder,
  STALE_AFTER_MS,
} from '../domain/demo'
import type { DemoState, FundKind, OrderInput, Scenario } from '../domain/demo'

// Browser-only simulation: no wallet provider, fetch, storage, or canister calls.
let state = initialState()
const emptyServerState = initialState()
const listeners = new Set<() => void>()
let generation = 0
let pending = new Map<string, ReturnType<typeof setTimeout>>()
/** Time the demo takes to "answer" a queued order. */
const SETTLE_DELAY_MS = 700
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
function clearPending(id: string) {
  const timer = pending.get(id)
  if (timer === undefined) return
  clearTimeout(timer)
  pending.delete(id)
}
export function useDemo() {
  return useSyncExternalStore(
    subscribe,
    () => state,
    () => emptyServerState,
  )
}
/** Snapshot for callers outside React (unit tests, imperative UI helpers). */
export function getDemoState(): DemoState {
  return state
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
  publish({ ...state, observedAt: Date.now() - (stale ? STALE_AFTER_MS + 1 : 0) })
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
      // Settle against the live state: a captured snapshot would drop orders and
      // fund moves made while the order was in flight.
      if (epoch === generation) publish(settleOrder(state, id, scenario))
    }, SETTLE_DELAY_MS),
  )
}
export function cancelDemo(id: string) {
  clearPending(id)
  publish(cancelOrder(state, id))
}
export function cancelAllDemo() {
  state.orders.forEach((order) => clearPending(order.id))
  publish(state.orders.reduce((next, order) => cancelOrder(next, order.id), state))
}
export function transferDemo(id: string, kind: FundKind, amount: number) {
  const { state: next, error } = moveFunds(
    state,
    id,
    kind,
    amount,
    new Date().toLocaleTimeString('ja-JP'),
  )
  // Publish first: a rejected request is recorded so the same id cannot be reused.
  publish(next)
  if (error) throw new Error(error)
}
