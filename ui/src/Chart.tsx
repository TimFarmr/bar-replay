import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import {
  init,
  dispose,
  type Chart as KLineChart,
  type DataLoaderGetBarsParams,
  type DataLoaderSubscribeBarParams,
  type DeepPartial,
  type KLineData,
  type Styles,
} from 'klinecharts'
import type { Candle } from './types'

/**
 * KLineCharts v10 has no `applyNewData`; data is pulled through a loader and
 * updated through a subscription. That maps onto bar replay exactly:
 *
 * - `getBars({type:'init'})` serves the window the backend already filtered to
 *   the cursor;
 * - `subscribeBar` hands us a callback that appends a candle, or replaces the
 *   one with the same timestamp. Stepping forward does one or the other, so
 *   `push` keeps the user's zoom and pan intact.
 *
 * Only moves that *remove* candles - stepping back, jumping, changing
 * timeframe - need `reload`, because no incremental update can express a
 * candle disappearing.
 */

export interface ChartHandle {
  /** Replace everything. Resets the viewport. */
  reload(candles: Candle[], priceDecimals: number): void
  /** Append or update the newest candles without disturbing the viewport. */
  push(candles: Candle[]): void
}

const TICKER = 'BAR-REPLAY'

/** The base series is 1-minute; aggregation happens in Rust (ADR 0013). */
const PERIOD = { type: 'minute', span: 1 } as const

const DARK: DeepPartial<Styles> = {
  grid: {
    horizontal: { color: '#1b2430' },
    vertical: { color: '#1b2430' },
  },
  candle: {
    bar: {
      upColor: '#26a69a',
      downColor: '#ef5350',
      upBorderColor: '#26a69a',
      downBorderColor: '#ef5350',
      upWickColor: '#26a69a',
      downWickColor: '#ef5350',
    },
    tooltip: { offsetTop: 6 },
  },
  xAxis: {
    axisLine: { color: '#2a3441' },
    tickLine: { color: '#2a3441' },
    tickText: { color: '#8a94a6' },
  },
  yAxis: {
    axisLine: { color: '#2a3441' },
    tickLine: { color: '#2a3441' },
    tickText: { color: '#8a94a6' },
  },
  crosshair: {
    horizontal: { line: { color: '#6b7787' }, text: { backgroundColor: '#2a3441' } },
    vertical: { line: { color: '#6b7787' }, text: { backgroundColor: '#2a3441' } },
  },
}

function toKLine(c: Candle): KLineData {
  return {
    timestamp: c.ts,
    open: c.open,
    high: c.high,
    low: c.low,
    close: c.close,
    volume: c.volume,
  }
}

export const Chart = forwardRef<ChartHandle, { priceDecimals: number; sessionTz: string }>(
  function Chart({ priceDecimals, sessionTz }, ref) {
    const containerRef = useRef<HTMLDivElement>(null)
    const chartRef = useRef<KLineChart | null>(null)
    /** The loader closes over this, so a reload is a ref write plus a reset. */
    const candlesRef = useRef<Candle[]>([])
    /** Set once the chart subscribes; null before that. */
    const pushRef = useRef<((d: KLineData) => void) | null>(null)
    const decimalsRef = useRef(priceDecimals)

    useEffect(() => {
      const container = containerRef.current
      if (container === null) return

      const chart = init(container, { styles: DARK })
      if (chart === null) return
      chartRef.current = chart

      // Spec the data policy: the axis reads in the instrument's session timezone, which
      // the UI also names. Left unset, KLineCharts would quietly render the
      // machine's local time under a label saying otherwise.
      chart.setTimezone(sessionTz)
      chart.setPeriod(PERIOD)
      chart.setDataLoader({
        getBars: ({ type, callback }: DataLoaderGetBarsParams) => {
          // The backend always sends the whole visible window, so there is no
          // older page to fetch: say so, or the chart keeps asking.
          callback(type === 'init' ? candlesRef.current.map(toKLine) : [], false)
        },
        subscribeBar: ({ callback }: DataLoaderSubscribeBarParams) => {
          pushRef.current = callback
        },
        unsubscribeBar: () => {
          pushRef.current = null
        },
      })

      return () => {
        pushRef.current = null
        chartRef.current = null
        dispose(container)
      }
    }, [sessionTz])

    useImperativeHandle(
      ref,
      () => ({
        reload(candles: Candle[], decimals: number) {
          candlesRef.current = candles
          decimalsRef.current = decimals
          // setSymbol resets the data and re-invokes the loader. It also
          // carries the precision, so data and decimals take one path.
          chartRef.current?.setSymbol({ ticker: TICKER, pricePrecision: decimals })
        },
        push(candles: Candle[]) {
          const send = pushRef.current
          if (send === null) {
            // Not subscribed yet: fall back to a reload so a step is never
            // silently dropped.
            candlesRef.current = mergeTail(candlesRef.current, candles)
            chartRef.current?.setSymbol({
              ticker: TICKER,
              pricePrecision: decimalsRef.current,
            })
            return
          }
          candlesRef.current = mergeTail(candlesRef.current, candles)
          for (const c of candles) send(toKLine(c))
        },
      }),
      [],
    )

    return <div className="chart" ref={containerRef} />
  },
)

/** Replace any candle the tail also covers, then append the rest. */
function mergeTail(existing: Candle[], tail: Candle[]): Candle[] {
  if (tail.length === 0) return existing
  const cut = existing.findIndex((c) => c.ts >= tail[0].ts)
  const head = cut === -1 ? existing : existing.slice(0, cut)
  return [...head, ...tail]
}
