import { CandlestickSeries, ColorType, createChart, type UTCTimestamp } from 'lightweight-charts'
import { useEffect, useRef, useState } from 'react'
import { MarketFeed, type Candle, type MarketSnapshot } from '../client/market-feed'

const feed = new MarketFeed()

export function useMarket(): MarketSnapshot {
  const [snapshot, setSnapshot] = useState<MarketSnapshot>({
    connection: 'connecting',
    mids: {},
    book: {},
    trades: {},
    candles: {},
  })
  useEffect(() => {
    feed.start()
    const unsubscribe = feed.subscribe(setSnapshot)
    return () => {
      unsubscribe()
      feed.stop()
    }
  }, [])
  return snapshot
}

export function PriceChart({ candles }: { candles: Candle[] }) {
  const element = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!element.current) return
    const chart = createChart(element.current, {
      height: 330,
      layout: { background: { type: ColorType.Solid, color: '#101a20' }, textColor: '#78909b' },
      grid: { vertLines: { color: '#18262d' }, horzLines: { color: '#18262d' } },
      rightPriceScale: { borderColor: '#293940' },
      timeScale: { borderColor: '#293940', timeVisible: true },
    })
    const series = chart.addSeries(CandlestickSeries, {
      upColor: '#8ce3c5',
      downColor: '#da7a87',
      borderVisible: false,
      wickUpColor: '#8ce3c5',
      wickDownColor: '#da7a87',
    })
    series.setData(
      candles.map((item) => ({
        time: Math.floor(item.t / 1000) as UTCTimestamp,
        open: Number(item.o),
        high: Number(item.h),
        low: Number(item.l),
        close: Number(item.c),
      })),
    )
    chart.timeScale().fitContent()
    const observer = new ResizeObserver(([entry]) =>
      chart.applyOptions({ width: entry.contentRect.width }),
    )
    observer.observe(element.current)
    return () => {
      observer.disconnect()
      chart.remove()
    }
  }, [candles])
  return <div className="price-chart" ref={element} aria-label="価格チャート" />
}
