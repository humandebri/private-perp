import { describe, expect, it, vi } from 'vitest'
import { reduceMarketMessage, type MarketSnapshot } from '../../src/client/market-feed'

const base = (): MarketSnapshot => ({
  connection: 'connecting',
  mids: {},
  book: {},
  trades: {},
  candles: {},
})

describe('public market reducer', () => {
  it('replaces book snapshots and never needs a user identifier', () => {
    vi.setSystemTime(new Date('2026-09-22T00:00:00Z'))
    const next = reduceMarketMessage(base(), {
      channel: 'l2Book',
      data: { coin: 'BTC', isSnapshot: true, levels: [[{ px: '1', sz: '2', n: 1 }], []] },
    })
    expect(next.book.BTC[0][0].px).toBe('1')
    expect(next.connection).toBe('live')
  })

  it('deduplicates candles by open time', () => {
    const one = reduceMarketMessage(base(), {
      channel: 'candle',
      data: { s: 'BTC', t: 1, o: '1', h: '2', l: '1', c: '1' },
    })
    const two = reduceMarketMessage(one, {
      channel: 'candle',
      data: { s: 'BTC', t: 1, o: '1', h: '3', l: '1', c: '2' },
    })
    expect(two.candles.BTC).toHaveLength(1)
    expect(two.candles.BTC[0].c).toBe('2')
  })
})
