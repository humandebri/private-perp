import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  MarketFeed,
  mergeCandleHistory,
  validCandle,
  reduceMarketMessage,
  type MarketSnapshot,
} from '../../src/client/market-feed'
import { marketInfoUrl } from '../../src/market-endpoint'

vi.mock('../../src/client/config', () => ({
  resolveConfig: () => ({ marketWs: 'wss://api.hyperliquid-testnet.xyz/ws' }),
}))

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

  it('rejects unusable timestamps, prices, OHLC ranges and other intervals', () => {
    const candle = { t: 60_000, o: '2', h: '3', l: '1', c: '2' }
    for (const patch of [
      { t: NaN },
      { t: -1 },
      { t: 1.5 },
      { c: 'NaN' },
      { h: 'Infinity' },
      { o: '0' },
      { h: '1' },
      { l: '3' },
    ])
      expect(validCandle({ ...candle, ...patch })).toBe(false)
    expect(
      reduceMarketMessage(base(), { channel: 'candle', data: { ...candle, s: 'BTC', i: '1h' } })
        .candles,
    ).toEqual({})
  })

  it('refreshes cached bars while preserving live updates received during history loading', () => {
    const cached = { t: 60_000, o: '1', h: '4', l: '1', c: '2' }
    const updated = { ...cached, c: '3' }
    const older = { ...cached, t: 1 }
    const history = [{ ...cached, c: '4' }, older, older]
    expect(mergeCandleHistory([cached], history, [cached])).toEqual([older, history[0]])
    expect(mergeCandleHistory([updated], history, [cached])).toEqual([older, updated])
    const newerHistory = { ...cached, c: '4', n: 10 }
    expect(mergeCandleHistory([{ ...updated, n: 9 }], [newerHistory], [cached])).toEqual([
      newerHistory,
    ])
    expect(
      mergeCandleHistory(
        [],
        Array.from({ length: 5001 }, (_, t) => ({ ...cached, t: t + 1 })),
        [],
      ),
    ).toHaveLength(5000)
  })

  it('uses the configured venue for history without falling back to mainnet', () => {
    expect(marketInfoUrl('wss://api.hyperliquid-testnet.xyz/ws')).toBe(
      'https://api.hyperliquid-testnet.xyz/info',
    )
    expect(marketInfoUrl('ws://127.0.0.1:8080/ws?token=x')).toBe('http://127.0.0.1:8080/info')
    expect(() => marketInfoUrl('https://example.com')).toThrow()
  })

  it('accepts candle batches, sorting each symbol and ignoring malformed bars', () => {
    const candle = { s: 'BTC', i: '1m', t: 60_000, o: '2', h: '3', l: '1', c: '2' }
    const next = reduceMarketMessage(base(), {
      channel: 'candle',
      data: [{ ...candle, t: 120_000 }, candle, { ...candle, h: '0' }, { ...candle, s: 'ETH' }],
    })
    expect(next.candles.BTC.map((item) => item.t)).toEqual([60_000, 120_000])
    expect(next.candles.ETH).toHaveLength(1)
  })
})

describe('candle history lifecycle', () => {
  let feed: MarketFeed | undefined
  afterEach(() => {
    feed?.stop()
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  function setup(fetcher: typeof fetch) {
    vi.useFakeTimers()
    vi.stubGlobal('navigator', {})
    vi.stubGlobal(
      'BroadcastChannel',
      class {
        postMessage() {}
        close() {}
      },
    )
    vi.stubGlobal(
      'WebSocket',
      class {
        close() {}
      },
    )
    vi.stubGlobal('fetch', fetcher)
    feed = new MarketFeed()
    let snapshot = base()
    feed.subscribe((next) => {
      snapshot = next
    })
    feed.start()
    return () => snapshot
  }

  it('loads both symbols from the real read-only info endpoint', async () => {
    const requests: { url: string; body: any }[] = []
    const current = setup(
      vi.fn(async (url, init) => {
        const body = JSON.parse(String(init?.body))
        requests.push({ url: String(url), body })
        return new Response(
          JSON.stringify([
            { s: body.req.coin, i: '1m', t: 60_000, o: '2', h: '3', l: '1', c: '2' },
          ]),
        )
      }),
    )
    await vi.waitFor(() => expect(current().candleHistory).toEqual({ BTC: 'ready', ETH: 'ready' }))
    expect(requests.map((r) => r.body.req.coin)).toEqual(['BTC', 'ETH'])
    expect(
      requests.every(
        (r) =>
          r.url === 'https://api.hyperliquid-testnet.xyz/info' &&
          r.body.type === 'candleSnapshot' &&
          r.body.req.interval === '1m' &&
          r.body.req.endTime - r.body.req.startTime === 86_400_000,
      ),
    ).toBe(true)
    expect(current().candles.BTC[0].c).toBe('2')
  })

  it('reports HTTP failure without generating substitute candles', async () => {
    const current = setup(vi.fn(async () => new Response('Unavailable', { status: 503 })))
    await vi.waitFor(() => expect(current().candleHistory?.BTC).toBe('error'))
    expect(current().candles).toEqual({})
  })

  it('rejects a response for the wrong symbol or interval', async () => {
    const current = setup(
      vi.fn(
        async () =>
          new Response(
            JSON.stringify([{ s: 'SOL', i: '1h', t: 60_000, o: '2', h: '3', l: '1', c: '2' }]),
          ),
      ),
    )
    await vi.waitFor(() => expect(current().candleHistory?.BTC).toBe('error'))
    expect(current().candles).toEqual({})
  })

  it('ignores a delayed history response after the feed stops', async () => {
    let finish: ((response: Response) => void) | undefined
    const current = setup(
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            finish = resolve
          }),
      ),
    )
    feed!.stop()
    finish!(
      new Response(
        JSON.stringify([{ s: 'ETH', i: '1m', t: 60_000, o: '2', h: '3', l: '1', c: '2' }]),
      ),
    )
    await Promise.resolve()
    await Promise.resolve()
    expect(current().candles).toEqual({})
  })
})
