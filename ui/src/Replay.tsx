import { useEffect, useRef, useState } from 'react'
import { Chart, type ChartHandle } from './Chart'
import { Trading } from './Trading'
import { Review } from './Review'
import { api, dateToMs, msToDate } from './api'
import { TIMEFRAMES, type Gap, type View } from './types'

/** Bars revealed per second in play mode. */
const SPEEDS = [1, 2, 5, 10, 25] as const

export function Replay({ initial, onExit }: { initial: View; onExit: () => void }) {
  const [view, setView] = useState<View>(initial)
  const [playing, setPlaying] = useState(false)
  const [speed, setSpeed] = useState<number>(5)
  const [jump, setJump] = useState(() => msToDate(initial.cursor.cursor))
  const [error, setError] = useState('')
  const [reviewing, setReviewing] = useState(false)

  const chartRef = useRef<ChartHandle>(null)
  /** Guards against a second step starting before the last one answered. */
  const stepping = useRef(false)

  /** Any move that can remove candles reloads the whole window. */
  function applyFull(next: View) {
    setView(next)
    setJump(msToDate(next.cursor.cursor))
    chartRef.current?.reload(next.candles, next.instrument.priceDecimals)
  }

  useEffect(() => {
    applyFull(initial)
    // Only on mount: later updates go through the handlers below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function forward(count: number) {
    if (stepping.current) return
    stepping.current = true
    try {
      const step = await api.stepForward(count)
      // Incremental: keeps the user's zoom and pan (see Chart.tsx).
      chartRef.current?.push(step.tail)
      // Stepping can fire a stop or a target, so trading state comes with it.
      // Newly revealed gaps are merged in rather than waiting for a reload, so
      // a hole in the data is flagged the moment playback crosses it.
      setView((v) => ({
        ...v,
        cursor: step.cursor,
        trading: step.trading,
        gaps: step.newGaps.length === 0 ? v.gaps : merge(v.gaps, step.newGaps),
      }))
      setJump(msToDate(step.cursor.cursor))
      if (step.cursor.atEnd) setPlaying(false)
    } catch (e) {
      setError(String(e))
      setPlaying(false)
    } finally {
      stepping.current = false
    }
  }

  async function back(count: number) {
    setPlaying(false)
    try {
      applyFull(await api.stepBack(count))
    } catch (e) {
      setError(String(e))
    }
  }

  async function changeTimeframe(tf: string) {
    setPlaying(false)
    try {
      applyFull(await api.setTimeframe(tf))
    } catch (e) {
      setError(String(e))
    }
  }

  async function goTo(date: string) {
    setPlaying(false)
    try {
      applyFull(await api.jumpTo(dateToMs(date)))
    } catch (e) {
      setError(String(e))
    }
  }

  useEffect(() => {
    if (!playing) return
    const id = setInterval(() => void forward(1), 1000 / speed)
    return () => clearInterval(id)
  }, [playing, speed])

  // Keyboard transport: the controls a replay actually needs under the hand.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return
      if (e.key === 'ArrowRight') void forward(e.shiftKey ? 10 : 1)
      else if (e.key === 'ArrowLeft') void back(e.shiftKey ? 10 : 1)
      else if (e.key === ' ') {
        e.preventDefault()
        setPlaying((p) => !p)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const { cursor, instrument, gaps } = view
  const dataGaps = gaps.filter((g) => g.kind === 'data')
  const progress = cursor.bars <= 1 ? 100 : ((cursor.position - 1) / (cursor.bars - 1)) * 100

  return (
    <div className="replay">
      <header className="bar">
        <button onClick={onExit}>← Sessions</button>
        <strong>{instrument.symbol}</strong>
        <span className="muted">{instrument.provider}</span>

        <span className="tfs">
          {TIMEFRAMES.map((tf) => (
            <button
              key={tf}
              className={tf === view.timeframe ? 'tf on' : 'tf'}
              onClick={() => void changeTimeframe(tf)}
            >
              {tf}
            </button>
          ))}
        </span>

        <span className="spacer" />
        <span
          className={instrument.spreadMode === 'historical' ? 'badge good' : 'badge warn'}
          title={
            instrument.spreadMode === 'historical'
              ? 'Spread comes from the provider’s real bid/ask.'
              : 'This provider has no bid/ask. Any spread is a value you set, not historical fact.'
          }
        >
          {instrument.spreadMode === 'historical' ? 'real spread' : 'synthetic spread'}
        </span>
        <span className="badge">{instrument.sessionTz}</span>
        <button onClick={() => setReviewing(true)}>Review</button>
      </header>

      {reviewing && <Review onClose={() => setReviewing(false)} onChange={applyFull} />}

      <div className="middle">
        <div className="chart-wrap">
          <Chart
            ref={chartRef}
            priceDecimals={instrument.priceDecimals}
            sessionTz={instrument.sessionTz}
          />
        </div>
        <Trading
          trading={view.trading}
          instrument={instrument}
          onChange={applyFull}
          onError={setError}
        />
      </div>

      {dataGaps.length > 0 && (
        <div className="gap-warning">
          ⚠ {dataGaps.length} data gap{dataGaps.length > 1 ? 's' : ''} in view —{' '}
          {dataGaps[0].reason}. Missing bars are never filled in; the chart skips
          them.
        </div>
      )}

      <footer className="bar controls">
        <button onClick={() => void back(10)} disabled={cursor.atStart}>
          ⏪ 10
        </button>
        <button onClick={() => void back(1)} disabled={cursor.atStart}>
          ◀ step
        </button>
        <button
          className="primary"
          onClick={() => setPlaying((p) => !p)}
          disabled={cursor.atEnd}
        >
          {playing ? '❚❚ pause' : '▶ play'}
        </button>
        <button onClick={() => void forward(1)} disabled={cursor.atEnd}>
          step ▶
        </button>
        <button onClick={() => void forward(10)} disabled={cursor.atEnd}>
          10 ⏩
        </button>

        <select value={speed} onChange={(e) => setSpeed(Number(e.target.value))}>
          {SPEEDS.map((s) => (
            <option key={s} value={s}>
              {s} bars/s
            </option>
          ))}
        </select>

        <span className="spacer" />

        <label className="jump">
          Jump to
          <input type="date" value={jump} onChange={(e) => void goTo(e.target.value)} />
        </label>

        <span className="cursor" title="The replay cursor: nothing after this is known.">
          {cursor.cursorLabel} {cursor.timezone} · bar {cursor.position.toLocaleString()} /{' '}
          {cursor.bars.toLocaleString()}
        </span>
      </footer>

      <div className="progress">
        <div className="progress-fill" style={{ width: `${progress}%` }} />
      </div>

      {error !== '' && <p className="error">{error}</p>}
    </div>
  )
}

/** Gaps are identified by where they start; a step can re-report one. */
function merge(existing: Gap[], incoming: Gap[]): Gap[] {
  const seen = new Set(existing.map((g) => g.from))
  return [...existing, ...incoming.filter((g) => !seen.has(g.from))]
}
