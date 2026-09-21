import { describe, expect, it } from 'vitest'
import {
  acceptOrder,
  cancelOrder,
  fromScaledInteger,
  halveQuantity,
  initialState,
  moveFunds,
  orderFingerprint,
  parseUsdc,
  settleOrder,
  toScaledInteger,
} from '../../src/domain/demo'
import type { FundKind, OrderInput } from '../../src/domain/demo'
const input: OrderInput = {
  market: 'BTC',
  side: 'buy',
  kind: 'Limit',
  quantity: '0.01',
  price: '64000',
}
const active = () => ({ ...initialState(), active: true, observedAt: 1000, reserve: 1_000_000_000 })
const fund = (state: ReturnType<typeof active>, id: string, kind: FundKind, amount: number) =>
  moveFunds(state, id, kind, amount, 'now')
describe('demo order state (not a production ledger)', () => {
  it('deduplicates the same request and rejects changed contents', () => {
    const state = acceptOrder(active(), input, 'one', 1000)
    expect(acceptOrder(state, input, 'one', 1000)).toBe(state)
    expect(() => acceptOrder(state, { ...input, quantity: '1' }, 'one', 1000)).toThrow('同一ID')
  })
  it('identifies a repeated order no matter how the input keys are ordered', () => {
    const state = acceptOrder(active(), input, 'one', 1000)
    const reordered: OrderInput = {
      price: input.price,
      quantity: input.quantity,
      kind: input.kind,
      side: input.side,
      market: input.market,
    }
    expect(orderFingerprint(reordered)).toBe(orderFingerprint(input))
    expect(acceptOrder(state, reordered, 'one', 1000)).toBe(state)
  })
  it('rejects unauthenticated, stale and invalid orders', () => {
    expect(() => acceptOrder(initialState(), input, 'one', 1000)).toThrow()
    expect(() => acceptOrder(active(), input, 'one', 1000 + 10_001)).toThrow('古い')
    for (const quantity of ['0', '-1', 'NaN', 'Infinity', '1e6', '0.000001'])
      expect(() => acceptOrder(active(), { ...input, quantity }, 'one', 1000)).toThrow()
  })
  it('keeps unknown even on cancellation or repeat reconciliation', () => {
    const unknown = settleOrder(acceptOrder(active(), input, 'one', 1000), 'one', 'unknown')
    expect(cancelOrder(unknown, 'one').orders[0].status).toBe('unknown')
    expect(settleOrder(unknown, 'one', 'open').orders[0].status).toBe('unknown')
  })
  it('preserves partial fills when cancelled', () => {
    const partial = settleOrder(acceptOrder(active(), input, 'one', 1000), 'one', 'partial')
    const cancelled = cancelOrder(partial, 'one').orders[0]
    expect(cancelled.status).toBe('cancelled')
    expect(cancelled.filled).toBe('0.005')
  })
  it('represents a fill winning the cancellation race', () => {
    const open = settleOrder(
      acceptOrder({ ...active(), scenario: 'cancel-race' }, input, 'one', 1000),
      'one',
      'open',
    )
    expect(cancelOrder(open, 'one').orders[0]).toMatchObject({
      status: 'filled',
      filled: '0.01',
    })
  })
  it('ignores stale settlement callbacks after pre-dispatch cancellation', () => {
    const cancelled = cancelOrder(acceptOrder(active(), input, 'one', 1000), 'one')
    expect(settleOrder(cancelled, 'one', 'open').orders[0].status).toBe('cancelled')
  })
  it('never fills a queued order that is cancelled, whatever the panel scenario says', () => {
    const queued = acceptOrder({ ...active(), scenario: 'cancel-race' }, input, 'one', 1000)
    expect(cancelOrder(queued, 'one').orders[0]).toMatchObject({
      status: 'cancelled',
      filled: '0',
    })
  })
  it('cancels an order whose own scenario is not the race', () => {
    const open = settleOrder(acceptOrder(active(), input, 'one', 1000), 'one', 'open')
    expect(cancelOrder(open, 'one').orders[0].status).toBe('cancelled')
  })
})
describe('exact decimal quantity helpers', () => {
  it('round-trips decimal strings through integer units', () => {
    expect(toScaledInteger('1.5', 5)).toBe(150_000)
    expect(fromScaledInteger(150_000, 5)).toBe('1.5')
    expect(fromScaledInteger(1, 5)).toBe('0.00001')
    for (const value of ['-1', '1e3', '0.0000001'])
      expect(() => toScaledInteger(value, 5)).toThrow()
  })
  it('halves by integer division and leaves the remainder untouched', () => {
    expect(halveQuantity('0.01')).toEqual({ filled: '0.005', remainder: '0.005' })
    expect(halveQuantity('0.1')).toEqual({ filled: '0.05', remainder: '0.05' })
    expect(halveQuantity('1')).toEqual({ filled: '0.5', remainder: '0.5' })
    expect(halveQuantity('0.00001')).toEqual({ filled: '0', remainder: '0.00001' })
    expect(halveQuantity('0.00003')).toEqual({ filled: '0.00001', remainder: '0.00002' })
    expect(halveQuantity('0.00005')).toEqual({ filled: '0.00002', remainder: '0.00003' })
  })
  it('keeps partial fills inside the 5-decimal quantity limit', () => {
    for (const quantity of ['0.00001', '0.00003', '0.00009', '0.00005', '0.12345', '0.01']) {
      const order = settleOrder(
        acceptOrder(active(), { ...input, quantity }, 'one', 1000),
        'one',
        'partial',
      ).orders[0]
      const { filled, remainder } = halveQuantity(quantity)
      expect(Number(filled) + Number(remainder)).toBeCloseTo(Number(quantity), 10)
      if (filled === '0') expect(order).toMatchObject({ status: 'open', filled: '0' })
      else {
        expect(order.status).toBe('partial')
        expect(order.filled).toMatch(/^\d+(\.\d{1,5})?$/)
        expect(order.filled).toBe(filled)
      }
    }
  })
})
describe('synthetic funds', () => {
  it('uses exact integer units', () => {
    expect(parseUsdc('1.000001')).toBe(1_000_001)
    for (const amount of ['-1', '0', '1e3', '1.0000001', 'Infinity'])
      expect(() => parseUsdc(amount)).toThrow()
  })
  it('conserves balances through allocation and recovery', () => {
    const allocated = fund(active(), 'a', 'allocate', 100_000_000)
    expect(allocated.error).toBeNull()
    expect(allocated.state.reserve + allocated.state.trading).toBe(1_000_000_000)
    const recovered = fund(allocated.state, 'r', 'recover', 100_000_000)
    expect(recovered.error).toBeNull()
    expect(recovered.state.reserve).toBe(1_000_000_000)
  })
  it('does not spend twice or silently reuse an id for a different request', () => {
    const withdrawn = fund(active(), 'w', 'withdraw', 100_000_000)
    expect(withdrawn.error).toBeNull()
    expect(withdrawn.state.reserve).toBe(900_000_000)
    const retried = fund(withdrawn.state, 'w', 'withdraw', 100_000_000)
    expect(retried.state).toBe(withdrawn.state)
    expect(retried.error).toBeNull()
    const changed = fund(withdrawn.state, 'w', 'withdraw', 200_000_000)
    expect(changed.error).toContain('同一ID')
    expect(changed.state).toBe(withdrawn.state)
  })
  it('records rejected requests so the id can never carry a different body', () => {
    const overdraw = 1_000_000_001
    const rejected = fund(active(), 'w2', 'withdraw', overdraw)
    expect(rejected.error).toContain('不足')
    expect(rejected.state.fundRequests).toEqual([
      { id: 'w2', kind: 'withdraw', amount: overdraw, error: rejected.error },
    ])
    expect(rejected.state.events).toHaveLength(0)
    // The same body repeats the recorded outcome and still applies nothing.
    const retried = fund(rejected.state, 'w2', 'withdraw', overdraw)
    expect(retried.error).toContain('不足')
    expect(retried.state.events).toHaveLength(0)
    expect(retried.state.fundRequests).toBe(rejected.state.fundRequests)
    // A different body reusing the id is refused outright.
    const changed = fund(rejected.state, 'w2', 'deposit', 1)
    expect(changed.error).toContain('同一ID')
    expect(changed.state).toBe(rejected.state)
    for (const amount of [0, -1, 1.5, Number.NaN])
      expect(fund(active(), 'bad', 'deposit', amount).error).toContain('範囲外')
  })
})
