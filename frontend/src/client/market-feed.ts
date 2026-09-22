import { resolveConfig } from './config'

export type BookLevel = { px: string; sz: string; n: number }
export type PublicTrade = {
  coin: string
  px: string
  sz: string
  side: string
  time: number
  tid: number
}
export type Candle = { t: number; o: string; h: string; l: string; c: string }
export type MarketSnapshot = {
  connection: 'connecting' | 'live' | 'reconnecting' | 'offline'
  mids: Record<string, string>
  book: Record<string, [BookLevel[], BookLevel[]]>
  trades: Record<string, PublicTrade[]>
  candles: Record<string, Candle[]>
  observedAt?: number
}

const initial = (): MarketSnapshot => ({
  connection: 'connecting',
  mids: {},
  book: {},
  trades: {},
  candles: {},
})

export function reduceMarketMessage(snapshot: MarketSnapshot, message: unknown): MarketSnapshot {
  if (!message || typeof message !== 'object') return snapshot
  const payload = message as { channel?: string; data?: any }
  const next = { ...snapshot, connection: 'live' as const, observedAt: Date.now() }
  if (payload.channel === 'allMids' && payload.data?.mids)
    return { ...next, mids: { ...snapshot.mids, ...payload.data.mids } }
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
  if (payload.channel === 'candle' && payload.data?.s) {
    const coin = payload.data.s as string
    const candle = payload.data as Candle
    const existing = snapshot.candles[coin] ?? []
    const withoutSame = existing.filter((item) => item.t !== candle.t)
    return {
      ...next,
      candles: {
        ...snapshot.candles,
        [coin]: [...withoutSame, candle].sort((a, b) => a.t - b.t).slice(-5000),
      },
    }
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
    const socket = new WebSocket(resolveConfig().marketWs)
    this.socket = socket
    socket.onopen = () => {
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
      try {
        this.snapshot = reduceMarketMessage(this.snapshot, JSON.parse(String(event.data)))
      } catch {
        return
      }
      this.publish()
    }
    socket.onclose = () => {
      if (this.heartbeat) clearInterval(this.heartbeat)
      if (this.stopped) return
      this.snapshot = { ...this.snapshot, connection: 'reconnecting' }
      this.publish()
      this.reconnect = setTimeout(() => this.openSocket(), 1000)
    }
    socket.onerror = () => socket.close()
  }

  private publish(): void {
    this.channel?.postMessage({ type: 'snapshot', snapshot: this.snapshot })
    this.emit()
  }

  private emit(): void {
    for (const listener of this.listeners) listener(this.snapshot)
  }
}
