import type { Candle } from './market-feed'

export function aggregateCandles(candles: Candle[], minutes: number): Candle[] {
  if (minutes === 1) return candles
  const interval = minutes * 60_000
  const buckets = new Map<number, Candle>()
  for (const candle of candles) {
    const t = Math.floor(candle.t / interval) * interval
    // Do not claim a complete historical bar if the loaded history starts mid-bucket.
    if (t < candles[0].t) continue
    const previous = buckets.get(t)
    buckets.set(
      t,
      previous
        ? {
            ...previous,
            h: String(Math.max(Number(previous.h), Number(candle.h))),
            l: String(Math.min(Number(previous.l), Number(candle.l))),
            c: candle.c,
            v: String(Number(previous.v ?? 0) + Number(candle.v ?? 0)),
            n: (previous.n ?? 0) + (candle.n ?? 0),
          }
        : { ...candle, t },
    )
  }
  return [...buckets.values()]
}
