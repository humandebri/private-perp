import { describe, expect, it } from 'vitest'
import { availableMargin, quoteOrder } from '../../src/client/order-ticket'
import { aggregateCandles } from '../../src/client/chart-data'

const args = {
  amount: '10',
  unit: 'margin' as const,
  reference: '85000',
  limit: '',
  kind: 'market' as const,
  side: 'buy' as const,
  leverage: 3,
  slippageBps: 50,
  sizeDecimals: 5,
}
describe('order ticket quotes', () => {
  it('uses exact decimal arithmetic and keeps the quoted margin within the requested budget', () => {
    const buy = quoteOrder(args)
    expect(buy).toEqual({
      quantity: '0.00035',
      price: '85425',
      notional: 29_898_750n,
      margin: 9_966_250n,
    })
    const sell = quoteOrder({ ...args, side: 'sell' })
    expect(sell.quantity).toBe('0.00035')
    expect(sell.margin).toBeLessThanOrEqual(10_000_000n)
    expect((Number(sell.quantity) * Number(args.reference)) / args.leverage).toBeLessThanOrEqual(10)
  })
  it('rounds price toward a stricter bound without widening slippage', () => {
    for (const side of ['buy', 'sell'] as const) {
      const quote = quoteOrder({ ...args, reference: '85234.7', side })
      const bound = 85234.7 * (side === 'buy' ? 1.005 : 0.995)
      if (side === 'buy') expect(Number(quote.price)).toBeLessThanOrEqual(bound)
      else expect(Number(quote.price)).toBeGreaterThanOrEqual(bound)
    }
  })
  it('preserves explicitly entered coin quantities and validates precision and limits', () => {
    expect(quoteOrder({ ...args, unit: 'coin', amount: '0.001' }).quantity).toBe('0.001')
    for (const patch of [
      { amount: 'NaN' },
      { amount: '0' },
      { amount: '1e6' },
      { leverage: 6 },
      { slippageBps: 0 },
      { sizeDecimals: 7 },
      { amount: '0.000001', unit: 'coin' as const },
    ])
      expect(() => quoteOrder({ ...args, ...patch })).toThrow()
    expect(() => quoteOrder({ ...args, kind: 'limit', limit: '85234.7' })).toThrow('precision')
    expect(quoteOrder({ ...args, kind: 'limit', limit: '85000' }).price).toBe('85000')
    const marketableSell = quoteOrder({ ...args, side: 'sell', kind: 'limit', limit: '50000' })
    expect(marketableSell.quantity).toBe('0.00035')
  })
  it('does not report used or reserved margin as available', () => {
    expect(availableMargin(100n, 30n, 25n)).toBe(45n)
    expect(availableMargin(100n, 80n, 30n)).toBe(0n)
  })
})
describe('real candle aggregation', () => {
  it('preserves open/close, extremes and volume and omits an incomplete first bucket', () => {
    const candle = { t: 60_000, o: '2', h: '3', l: '1', c: '2', v: '2', n: 1 }
    const bars = aggregateCandles(
      [candle, { ...candle, t: 300_000 }, { ...candle, t: 360_000, h: '4', c: '3', v: '5' }],
      5,
    )
    expect(bars).toEqual([{ ...candle, t: 300_000, h: '4', c: '3', v: '7', n: 2 }])
  })
})
