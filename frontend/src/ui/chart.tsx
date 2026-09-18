import { useEffect, useRef } from 'react'
import { CandlestickSeries, ColorType, createChart } from 'lightweight-charts'
import type { UTCTimestamp } from 'lightweight-charts'

export function Chart({ market }: { market: 'BTC' | 'ETH' }) {
  const container = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!container.current) return
    const chart = createChart(container.current, {
      autoSize: true,
      height: 355,
      layout: {
        background: { type: ColorType.Solid, color: '#0c1418' },
        textColor: '#7d939c',
        fontFamily: 'monospace',
      },
      grid: { vertLines: { color: '#142127' }, horzLines: { color: '#142127' } },
      rightPriceScale: { borderColor: '#223138' },
      timeScale: { borderColor: '#223138', timeVisible: true },
    })
    const series = chart.addSeries(CandlestickSeries, {
      upColor: '#8ce3c5',
      downColor: '#ee858e',
      borderVisible: false,
      wickUpColor: '#8ce3c5',
      wickDownColor: '#ee858e',
    })
    const base = market === 'BTC' ? 64380 : 3420
    const data = Array.from({ length: 90 }, (_, i) => {
      const open = base * (1 + Math.sin(i * 0.28) * 0.004 + i * 0.000035)
      const close = open + Math.sin(i * 1.8) * base * 0.0015
      return {
        time: (1789689600 + i * 900) as UTCTimestamp,
        open,
        close,
        high: Math.max(open, close) + base * 0.0008,
        low: Math.min(open, close) - base * 0.0006,
      }
    })
    series.setData(data)
    chart.timeScale().fitContent()
    return () => chart.remove()
  }, [market])
  return (
    <>
      <div
        ref={container}
        style={{ height: 355, width: '100%' }}
        aria-label={`${market} 合成ローソク足チャート`}
      />
      <a
        className="chart-credit"
        href="https://www.tradingview.com/"
        target="_blank"
        rel="noreferrer"
      >
        Charts by TradingView · 合成データ / 15分足
      </a>
    </>
  )
}
