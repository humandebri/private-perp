import {
  CandlestickSeries,
  HistogramSeries,
  LineStyle,
  ColorType,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
  type IPriceLine,
} from 'lightweight-charts'
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

export function PriceChart({
  candles,
  levels = [],
}: {
  candles: Candle[]
  levels?: { title: string; price: number; color: string }[]
}) {
  const element = useRef<HTMLDivElement>(null)
  const chartRef = useRef<IChartApi | null>(null)
  const seriesRef = useRef<ISeriesApi<'Candlestick'> | null>(null)
  const fitted = useRef(false)
  const volumeRef = useRef<ISeriesApi<'Histogram'> | null>(null)
  const lines = useRef<IPriceLine[]>([])
  const [hover, setHover] = useState<{ open: number; high: number; low: number; close: number }>()
  useEffect(() => {
    if (!element.current) return
    const chart = createChart(element.current, {
      height: 330,
      layout: { background: { type: ColorType.Solid, color: '#101a20' }, textColor: '#78909b' },
      grid: { vertLines: { color: '#18262d' }, horzLines: { color: '#18262d' } },
      rightPriceScale: { borderColor: '#293940' },
      timeScale: { borderColor: '#293940', timeVisible: true },
      localization: {
        locale: 'en-US',
        timeFormatter: (time: number) =>
          new Date(time * 1000).toLocaleString('en-US', { timeZone: 'UTC', hour12: false }),
      },
    })
    const series = chart.addSeries(CandlestickSeries, {
      upColor: '#8ce3c5',
      downColor: '#da7a87',
      borderVisible: false,
      wickUpColor: '#8ce3c5',
      wickDownColor: '#da7a87',
    })
    chartRef.current = chart
    seriesRef.current = series
    const volume = chart.addSeries(HistogramSeries, {
      priceFormat: { type: 'volume' },
      priceScaleId: 'volume',
    })
    volume.priceScale().applyOptions({ scaleMargins: { top: 0.82, bottom: 0 }, visible: false })
    series.priceScale().applyOptions({ scaleMargins: { top: 0.06, bottom: 0.22 } })
    volumeRef.current = volume
    chart.subscribeCrosshairMove((event) => {
      const value = event.seriesData.get(series)
      setHover(value && 'open' in value ? value : undefined)
    })
    const observer = new ResizeObserver(([entry]) =>
      chart.applyOptions({ width: entry.contentRect.width }),
    )
    observer.observe(element.current)
    return () => {
      observer.disconnect()
      chart.remove()
      chartRef.current = null
      seriesRef.current = null
      volumeRef.current = null
      lines.current = []
      fitted.current = false
    }
  }, [])
  useEffect(() => {
    seriesRef.current?.setData(
      candles.map((item) => ({
        time: Math.floor(item.t / 1000) as UTCTimestamp,
        open: Number(item.o),
        high: Number(item.h),
        low: Number(item.l),
        close: Number(item.c),
      })),
    )
    volumeRef.current?.setData(
      candles.map((item) => ({
        time: Math.floor(item.t / 1000) as UTCTimestamp,
        value: Number(item.v ?? 0),
        color: Number(item.c) >= Number(item.o) ? '#8ce3c550' : '#da7a8750',
      })),
    )
    if (!fitted.current && candles.length > 1) {
      chartRef.current
        ?.timeScale()
        .setVisibleLogicalRange({ from: Math.max(0, candles.length - 120), to: candles.length + 3 })
      fitted.current = true
    }
  }, [candles])
  useEffect(() => {
    const series = seriesRef.current
    if (!series) return
    lines.current.forEach((line) => series.removePriceLine(line))
    lines.current = levels
      .filter((level) => Number.isFinite(level.price) && level.price > 0)
      .map((level) =>
        series.createPriceLine({
          ...level,
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
        }),
      )
  }, [levels])
  const last = candles.at(-1)
  const values =
    hover ??
    (last && {
      open: Number(last.o),
      high: Number(last.h),
      low: Number(last.l),
      close: Number(last.c),
    })
  return (
    <>
      <div className="chart-ohlc" aria-label="Candle prices">
        {values &&
          Object.entries(values).map(([name, price]) => (
            <span key={name}>
              {name[0].toUpperCase()}{' '}
              <b>{price.toLocaleString('en-US', { maximumFractionDigits: 6 })}</b>
            </span>
          ))}
      </div>
      <div className="price-chart" ref={element} aria-label="Price chart" />
    </>
  )
}
