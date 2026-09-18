import { describe, expect, it } from 'vitest'
import {
  acceptOrder,
  cancelOrder,
  initialState,
  moveFunds,
  parseUsdc,
  settleOrder,
} from '../../src/domain/demo'
import type { OrderInput } from '../../src/domain/demo'
const input: OrderInput = {
  market: 'BTC',
  side: 'buy',
  kind: 'Limit',
  quantity: '0.01',
  price: '64000',
}
const active = () => ({ ...initialState(), active: true, observedAt: 1000, reserve: 1_000_000_000 })
describe('demo order state (not a production ledger)', () => {
  it('deduplicates the same request and rejects changed contents', () => {
    const state = acceptOrder(active(), input, 'one', 1000)
    expect(acceptOrder(state, input, 'one', 1000)).toBe(state)
    expect(() => acceptOrder(state, { ...input, quantity: '1' }, 'one', 1000)).toThrow('同一ID')
  })
  it('rejects unauthenticated, stale and invalid orders', () => {
    expect(() => acceptOrder(initialState(), input, 'one', 1000)).toThrow()
    expect(() => acceptOrder(active(), input, 'one', 12000)).toThrow('古い')
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
    const open = settleOrder(acceptOrder(active(), input, 'one', 1000), 'one', 'open')
    expect(cancelOrder(open, 'one', true).orders[0]).toMatchObject({
      status: 'filled',
      filled: '0.01',
    })
  })
  it('ignores stale settlement callbacks after pre-dispatch cancellation', () => {
    const cancelled = cancelOrder(acceptOrder(active(), input, 'one', 1000), 'one')
    expect(settleOrder(cancelled, 'one', 'open').orders[0].status).toBe('cancelled')
  })
})
describe('synthetic funds', () => {
  it('uses exact integer units', () => {
    expect(parseUsdc('1.000001')).toBe(1_000_001)
    for (const amount of ['-1', '0', '1e3', '1.0000001', 'Infinity'])
      expect(() => parseUsdc(amount)).toThrow()
  })
  it('conserves balances through allocation and recovery', () => {
    const allocated = moveFunds(active(), 'a', 'allocate', 100_000_000, 'now')
    expect(allocated.reserve + allocated.trading).toBe(1_000_000_000)
    const recovered = moveFunds(allocated, 'r', 'recover', 100_000_000, 'now')
    expect(recovered.reserve).toBe(1_000_000_000)
  })
  it('does not spend twice or silently reuse an id for a different request', () => {
    const withdrawn = moveFunds(active(), 'w', 'withdraw', 100_000_000, 'now')
    expect(moveFunds(withdrawn, 'w', 'withdraw', 100_000_000, 'now')).toBe(withdrawn)
    expect(() => moveFunds(withdrawn, 'w', 'withdraw', 200_000_000, 'now')).toThrow('同一ID')
    expect(() => moveFunds(withdrawn, 'w2', 'withdraw', 1_000_000_000, 'now')).toThrow('不足')
  })
})
