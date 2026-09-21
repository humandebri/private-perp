import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  cancelAllDemo,
  cancelDemo,
  getDemoState,
  logoutDemo,
  refreshDemo,
  setScenario,
  setStale,
  startDemo,
  submitDemo,
  transferDemo,
} from '../../src/client/demo-session'
import { initialState } from '../../src/domain/demo'
import type { OrderInput } from '../../src/domain/demo'

const input: OrderInput = {
  market: 'BTC',
  side: 'buy',
  kind: 'Limit',
  quantity: '0.01',
  price: '64000',
}
const order = (overrides: Partial<OrderInput> = {}) => ({ ...input, ...overrides })

beforeEach(() => {
  vi.useFakeTimers()
  // The store guards on `typeof window` before it will start a session.
  vi.stubGlobal('window', {})
  logoutDemo()
})
afterEach(() => {
  logoutDemo()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('demo session store (browser only)', () => {
  it('settles a queued order against the live state, not a stale snapshot', () => {
    startDemo()
    submitDemo(input, 'a')
    transferDemo('fund-1', 'allocate', 1_000_000)
    vi.advanceTimersByTime(700)
    const state = getDemoState()
    expect(state.orders.map((entry) => entry.id)).toEqual(['a'])
    expect(state.orders[0].status).toBe('open')
    // A captured pre-transfer snapshot would have reverted this balance.
    expect(state.reserve).toBe(5_000_000_000 - 1_000_000)
    expect(state.trading).toBe(10_000_000_000 + 1_000_000)
  })
  it('drops settlement callbacks from a replaced session', () => {
    startDemo()
    submitDemo(input, 'a')
    startDemo()
    const snapshot = getDemoState()
    vi.advanceTimersByTime(700)
    // Any publish would hand back a new object even if nothing changed.
    expect(getDemoState()).toBe(snapshot)
    expect(getDemoState().orders).toEqual([])
  })
  it('clears pending settlement when the session logs out', () => {
    startDemo()
    submitDemo(input, 'a')
    logoutDemo()
    const snapshot = getDemoState()
    vi.advanceTimersByTime(700)
    expect(getDemoState()).toBe(snapshot)
    expect(getDemoState()).toEqual(initialState())
  })
  it('queues one settlement per request id, not one per resend', () => {
    startDemo()
    submitDemo(input, 'a')
    const afterSubmit = getDemoState()
    submitDemo(input, 'a')
    expect(getDemoState()).toBe(afterSubmit)
    vi.advanceTimersByTime(700)
    const settled = getDemoState()
    expect(settled.orders[0].status).toBe('open')
    vi.advanceTimersByTime(700)
    // A second timer would have published again at 1400ms.
    expect(getDemoState()).toBe(settled)
  })
  it('cancels a queued order without a fill even when the panel simulates a race', () => {
    startDemo()
    setScenario('cancel-race')
    submitDemo(input, 'a')
    cancelDemo('a')
    expect(getDemoState().orders[0]).toMatchObject({ status: 'cancelled', filled: '0' })
    const snapshot = getDemoState()
    vi.advanceTimersByTime(700)
    expect(getDemoState()).toBe(snapshot)
    expect(getDemoState().orders[0].status).toBe('cancelled')
  })
  it('keeps each order on its own scenario when the panel changes later', () => {
    startDemo()
    submitDemo(input, 'a')
    vi.advanceTimersByTime(700)
    setScenario('cancel-race')
    cancelDemo('a')
    // The order was dispatched under 'open', so cancellation must win.
    expect(getDemoState().orders[0].status).toBe('cancelled')
  })
  it('fills a dispatched order that loses the cancellation race', () => {
    startDemo()
    setScenario('cancel-race')
    submitDemo(input, 'a')
    vi.advanceTimersByTime(700)
    cancelDemo('a')
    expect(getDemoState().orders[0]).toMatchObject({ status: 'filled', filled: '0.01' })
  })
  it('cancels every order through the same path', () => {
    startDemo()
    setScenario('cancel-race')
    submitDemo(input, 'a')
    submitDemo(order({ quantity: '0.02' }), 'b')
    cancelAllDemo()
    expect(getDemoState().orders.map((entry) => entry.status)).toEqual(['cancelled', 'cancelled'])
    const snapshot = getDemoState()
    vi.advanceTimersByTime(700)
    expect(getDemoState()).toBe(snapshot)
  })
  it('publishes a rejected fund request so its id cannot be reused', () => {
    startDemo()
    expect(() => transferDemo('fund-1', 'withdraw', 6_000_000_000)).toThrow('不足')
    expect(getDemoState().fundRequests).toHaveLength(1)
    expect(getDemoState().events).toHaveLength(0)
    expect(getDemoState().reserve).toBe(5_000_000_000)
    expect(() => transferDemo('fund-1', 'withdraw', 1)).toThrow('同一ID')
    expect(() => transferDemo('fund-1', 'withdraw', 6_000_000_000)).toThrow('不足')
  })
  it('blocks new orders while the account data is stale and recovers on refresh', () => {
    startDemo()
    setStale(true)
    expect(() => submitDemo(input, 'a')).toThrow('古い')
    setStale(false)
    expect(() => submitDemo(input, 'a')).not.toThrow()
    refreshDemo()
    expect(getDemoState().orders).toHaveLength(1)
  })
})
