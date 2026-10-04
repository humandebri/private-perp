import { resolveConfig } from './config'
import { marketInfoUrl } from '../market-endpoint'

export type BookLevel = { px: string; sz: string; n: number }
export type PublicTrade = {
  coin: string
  px: string
  sz: string
  side: string
  time: number
  tid: number
}
export type Candle = {
  t: number
  o: string
  h: string
  l: string
  c: string
  s?: string
  i?: string
  n?: number
  v?: string
}
export type MarketSnapshot = {
  connection: 'connecting' | 'live' | 'reconnecting' | 'offline'
  mids: Record<string, string>
  book: Record<string, [BookLevel[], BookLevel[]]>
  trades: Record<string, PublicTrade[]>
  candles: Record<string, Candle[]>
  candleHistory?: Record<string, 'loading' | 'ready' | 'error'>
  observedAt?: number
  midsObservedAt?: number
  assets?: Record<string, { sizeDecimals: number; maxLeverage: number }>
  assetContext?: Record<
    string,
    { markPx: string; funding: string; prevDayPx: string; dayNtlVlm: string }
  >
}

const initial = (): MarketSnapshot => ({
  connection: 'connecting',
  mids: {},
  book: {},
  trades: {},
  candles: {},
})

export function validCandle(value: unknown): value is Candle {
  if (!value || typeof value !== 'object') return false
  const candle = value as Candle
  if (!Number.isSafeInteger(candle.t) || candle.t <= 0) return false
  const prices = [candle.o, candle.h, candle.l, candle.c]
  if (
    prices.some(
      (price) => typeof price !== 'string' || !Number.isFinite(Number(price)) || Number(price) <= 0,
    )
  )
    return false
  return (
    Number(candle.h) >= Math.max(Number(candle.o), Number(candle.c)) &&
    Number(candle.l) <= Math.min(Number(candle.o), Number(candle.c))
  )
}

/** Refresh cached bars without overwriting a newer in-flight live update. */
export function mergeCandleHistory(
  existing: Candle[],
  history: Candle[],
  atRequest: Candle[],
): Candle[] {
  const before = new Map(atRequest.map((candle) => [candle.t, candle]))
  const fetched = new Map(history.map((candle) => [candle.t, candle]))
  const liveUpdates = existing.filter((candle) => {
    if (before.get(candle.t) === candle) return false
    const historical = fetched.get(candle.t)
    // A later REST snapshot may contain more trades than an earlier WS update.
    if (historical?.n !== undefined && candle.n !== undefined) return candle.n >= historical.n
    return true
  })
  return [
    ...new Map(
      [...existing, ...history, ...liveUpdates].map((candle) => [candle.t, candle]),
    ).values(),
  ]
    .sort((a, b) => a.t - b.t)
    .slice(-5000)
}

export function reduceMarketMessage(snapshot: MarketSnapshot, message: unknown): MarketSnapshot {
  if (!message || typeof message !== 'object') return snapshot
  const payload = message as { channel?: string; data?: any }
  const next = { ...snapshot, connection: 'live' as const, observedAt: Date.now() }
  if (payload.channel === 'activeAssetCtx' && payload.data?.coin && payload.data?.ctx) {
    return {
      ...next,
      assetContext: { ...snapshot.assetContext, [payload.data.coin]: payload.data.ctx },
    }
  }
  if (payload.channel === 'allMids' && payload.data?.mids)
    return { ...next, midsObservedAt: Date.now(), mids: { ...snapshot.mids, ...payload.data.mids } }
  if (payload.channel === 'l2Book' && payload.data?.coin && Array.isArray(payload.data.levels))
    return { ...next, book: { ...snapshot.book, [payload.data.coin]: payload.data.levels } }
  if (payload.channel === 'trades' && Array.isArray(payload.data)) {
    const coin = payload.data[0]?.coin
    if (!coin) return next
    return {
      ...next,
      trades: {
        ...snapshot.trades,
        [coin]: [...payload.data, ...(snapshot.trades[coin] ?? [])].slice(0, 40),
      },
    }
  }
  if (payload.channel === 'candle') {
    const updates: unknown[] = Array.isArray(payload.data) ? payload.data : [payload.data]
    const candles = { ...snapshot.candles }
    let changed = false
    for (const candle of updates) {
      if (!validCandle(candle) || !candle.s || (candle.i && candle.i !== '1m')) continue
      const existing = candles[candle.s] ?? []
      candles[candle.s] = [...existing.filter((item) => item.t !== candle.t), candle]
        .sort((a, b) => a.t - b.t)
        .slice(-5000)
      changed = true
    }
    return changed ? { ...next, candles } : snapshot
  }
  return next
}

type Listener = (snapshot: MarketSnapshot) => void

/** navigator.locksで1タブだけがWSを所有し、公開市況をBroadcastChannelへ配る。 */
export class MarketFeed {
  private snapshot = initial()
  private listeners = new Set<Listener>()
  private channel?: BroadcastChannel
  private socket?: WebSocket
  private stopped = false
  private releaseLeader?: () => void
  private reconnect?: ReturnType<typeof setTimeout>
  private heartbeat?: ReturnType<typeof setInterval>
  private election?: ReturnType<typeof setInterval>
  private isLeader = false
  private attemptingLeadership = false
  private lastReceivedAt = Date.now()
  private historyAbort?: AbortController

  start(): void {
    if (this.channel) return
    this.stopped = false
    this.channel = new BroadcastChannel('private-perp-public-market-v1')
    this.channel.onmessage = (event) => {
      if (event.data?.type === 'hello' && this.isLeader) return this.publish()
      if (event.data?.type !== 'snapshot') return
      this.lastReceivedAt = Date.now()
      this.snapshot = event.data.snapshot as MarketSnapshot
      this.emit()
    }
    this.channel.postMessage({ type: 'hello' })
    this.tryLeadership()
    this.election = setInterval(() => {
      if (!this.isLeader && Date.now() - this.lastReceivedAt > 3_000) this.tryLeadership()
    }, 2_000)
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener)
    listener(this.snapshot)
    return () => this.listeners.delete(listener)
  }

  stop(): void {
    this.stopped = true
    this.historyAbort?.abort()
    if (this.reconnect) clearTimeout(this.reconnect)
    if (this.heartbeat) clearInterval(this.heartbeat)
    if (this.election) clearInterval(this.election)
    this.socket?.close()
    this.channel?.close()
    this.releaseLeader?.()
    this.socket = undefined
    this.channel = undefined
    this.releaseLeader = undefined
    this.isLeader = false
    this.attemptingLeadership = false
    this.listeners.clear()
  }

  private tryLeadership(): void {
    if (this.stopped || this.isLeader || this.attemptingLeadership) return
    const locks = navigator.locks
    if (!locks) return this.openSocket()
    this.attemptingLeadership = true
    void locks.request('private-perp-market-feed', { ifAvailable: true }, async (lock) => {
      if (!lock || this.stopped) {
        this.attemptingLeadership = false
        return
      }
      this.isLeader = true
      this.openSocket()
      await new Promise<void>((resolve) => {
        this.releaseLeader = resolve
      })
      this.isLeader = false
      this.attemptingLeadership = false
    })
  }

  private openSocket(): void {
    if (this.stopped) return
    this.isLeader = true
    const marketWs = resolveConfig().marketWs
    const socket = new WebSocket(marketWs)
    this.socket = socket
    this.historyAbort?.abort()
    const controller = new AbortController()
    this.historyAbort = controller
    for (const coin of ['BTC', 'ETH']) void this.loadHistory(coin, marketWs, controller)
    socket.onopen = () => {
      if (this.stopped || this.socket !== socket) return
      void this.loadAssets(marketWs, controller)
      this.snapshot = { ...this.snapshot, connection: 'live' }
      for (const coin of ['BTC', 'ETH']) {
        for (const subscription of [
          { type: 'l2Book', coin },
          { type: 'trades', coin },
          { type: 'candle', coin, interval: '1m' },
          { type: 'bbo', coin },
          { type: 'activeAssetCtx', coin },
        ])
          socket.send(JSON.stringify({ method: 'subscribe', subscription }))
      }
      socket.send(JSON.stringify({ method: 'subscribe', subscription: { type: 'allMids' } }))
      this.heartbeat = setInterval(
        () =>
          socket.readyState === WebSocket.OPEN && socket.send(JSON.stringify({ method: 'ping' })),
        30_000,
      )
      this.publish()
    }
    socket.onmessage = (event) => {
      if (this.stopped || this.socket !== socket) return
      try {
        this.snapshot = reduceMarketMessage(this.snapshot, JSON.parse(String(event.data)))
      } catch {
        return
      }
      this.publish()
    }
    socket.onclose = () => {
      if (this.socket !== socket) return
      if (this.heartbeat) clearInterval(this.heartbeat)
      controller.abort()
      if (this.stopped) return
      this.snapshot = { ...this.snapshot, connection: 'reconnecting' }
      this.publish()
      this.reconnect = setTimeout(() => this.openSocket(), 1000)
    }
    socket.onerror = () => socket.close()
  }

  private async loadHistory(
    coin: string,
    marketWs: string,
    controller: AbortController,
  ): Promise<void> {
    const atRequest = this.snapshot.candles[coin] ?? []
    this.snapshot = {
      ...this.snapshot,
      candleHistory: { ...this.snapshot.candleHistory, [coin]: 'loading' },
    }
    this.publish()
    const endTime = Date.now()
    try {
      const response = await fetch(marketInfoUrl(marketWs), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          type: 'candleSnapshot',
          req: { coin, interval: '1m', startTime: endTime - 24 * 60 * 60_000, endTime },
        }),
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(15_000)]),
      })
      if (!response.ok) throw new Error('Candle history request failed')
      const data: unknown = await response.json()
      if (
        !Array.isArray(data) ||
        data.some((candle) => !validCandle(candle) || candle.s !== coin || candle.i !== '1m')
      )
        throw new Error('Invalid candle history')
      if (this.stopped || this.historyAbort !== controller || controller.signal.aborted) return
      this.snapshot = {
        ...this.snapshot,
        candles: {
          ...this.snapshot.candles,
          [coin]: mergeCandleHistory(this.snapshot.candles[coin] ?? [], data, atRequest),
        },
        candleHistory: { ...this.snapshot.candleHistory, [coin]: 'ready' },
      }
      this.publish()
    } catch {
      if (this.stopped || this.historyAbort !== controller || controller.signal.aborted) return
      this.snapshot = {
        ...this.snapshot,
        candleHistory: { ...this.snapshot.candleHistory, [coin]: 'error' },
      }
      this.publish()
    }
  }

  private publish(): void {
    this.channel?.postMessage({ type: 'snapshot', snapshot: this.snapshot })
    this.emit()
  }

  private emit(): void {
    for (const listener of this.listeners) listener(this.snapshot)
  }

  private async loadAssets(marketWs: string, controller: AbortController): Promise<void> {
    try {
      const response = await fetch(marketInfoUrl(marketWs), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ type: 'meta' }),
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(15_000)]),
      })
      if (!response.ok) return
      const data = (await response.json()) as {
        universe?: { name: string; szDecimals: number; maxLeverage: number }[]
      }
      if (
        !Array.isArray(data.universe) ||
        this.stopped ||
        this.historyAbort !== controller ||
        controller.signal.aborted
      )
        return
      const assets = Object.fromEntries(
        data.universe
          .filter(
            (asset) =>
              ['BTC', 'ETH'].includes(asset.name) &&
              Number.isInteger(asset.szDecimals) &&
              asset.szDecimals >= 0 &&
              asset.szDecimals <= 6 &&
              Number.isInteger(asset.maxLeverage) &&
              asset.maxLeverage > 0,
          )
          .map((asset) => [
            asset.name,
            { sizeDecimals: asset.szDecimals, maxLeverage: asset.maxLeverage },
          ]),
      )
      this.snapshot = { ...this.snapshot, assets }
      this.publish()
    } catch {
      /* The ticket stays disabled until venue precision is available. */
    }
  }
}
