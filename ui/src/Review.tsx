import { useEffect, useState } from 'react'
import { api } from './api'
import type { EquityPoint, Review as ReviewData, View } from './types'

/**
 * The review drawer: what actually happened, and what the trader thought at
 * the time.
 *
 * Everything is computed as of the cursor, so reviewing mid-session shows the
 * session so far rather than the finished result.
 */
export function Review({
  onClose,
  onChange,
}: {
  onClose: () => void
  onChange: (v: View) => void
}) {
  const [data, setData] = useState<ReviewData | null>(null)
  const [note, setNote] = useState('')
  const [tags, setTags] = useState('')
  const [exported, setExported] = useState('')
  const [error, setError] = useState('')

  function load() {
    api.review().then(setData).catch((e) => setError(String(e)))
  }

  useEffect(load, [])

  async function save() {
    if (note.trim() === '') return
    try {
      onChange(
        await api.addNote(
          null,
          note.trim(),
          tags
            .split(/[,\s]+/)
            .map((t) => t.trim())
            .filter((t) => t !== ''),
          null,
        ),
      )
      setNote('')
      setTags('')
      load()
    } catch (e) {
      setError(String(e))
    }
  }

  if (data === null) {
    return (
      <div className="drawer">
        <header className="bar">
          <strong>Review</strong>
          <span className="spacer" />
          <button onClick={onClose}>close</button>
        </header>
        <p className="hint">{error === '' ? 'Loading…' : error}</p>
      </div>
    )
  }

  const s = data.summary
  const d = data.priceDecimals
  const money = (v: number) => v.toFixed(2)

  return (
    <div className="drawer">
      <header className="bar">
        <strong>Review</strong>
        <span className="spacer" />
        <button
          onClick={() =>
            api
              .exportSession()
              .then(setExported)
              .catch((e) => setError(String(e)))
          }
        >
          Export CSV + JSON
        </button>
        <button onClick={onClose}>close</button>
      </header>

      <div className="drawer-body">
        {exported !== '' && (
          <p className="hint">
            Written to <code>{exported}</code> — local files only, nothing was
            uploaded.
          </p>
        )}
        {error !== '' && <p className="error">{error}</p>}

        <div className="stats">
          <Stat label="Net" value={money(s.net)} tone={s.net} />
          <Stat label="Trades" value={String(s.trades)} />
          <Stat label="Win rate" value={`${(s.winRate * 100).toFixed(1)}%`} />
          <Stat
            label="Profit factor"
            value={s.profitFactor === null ? '—' : s.profitFactor.toFixed(2)}
          />
          <Stat label="Expectancy" value={money(s.expectancy)} tone={s.expectancy} />
          <Stat
            label="Average R"
            value={s.averageR === null ? '—' : `${s.averageR.toFixed(2)}R`}
          />
          <Stat label="Max drawdown" value={money(s.maxDrawdown)} />
          <Stat label="Avg win / loss" value={`${money(s.averageWin)} / ${money(s.averageLoss)}`} />
        </div>

        {s.flaggedTrades > 0 && (
          <p className="gap-warning">
            ⚠ {s.flaggedTrades} trade{s.flaggedTrades > 1 ? 's' : ''} depended on
            the stop-first assumption: one bar touched both the stop and the
            target, and no finer data was available to say which came first.
            Treat those results as pessimistic, not factual.
          </p>
        )}

        <h3>Equity</h3>
        <EquityCurve points={data.equity} />

        <h3>Round trips</h3>
        {data.roundTrips.length === 0 ? (
          <p className="hint">No completed trades yet.</p>
        ) : (
          <table className="grid">
            <thead>
              <tr>
                <th>opened</th>
                <th>side</th>
                <th>qty</th>
                <th>entry</th>
                <th>exit</th>
                <th>R</th>
                <th>result</th>
              </tr>
            </thead>
            <tbody>
              {data.roundTrips.map((t, i) => (
                <tr key={i}>
                  <td className="muted">
                    {new Date(t.opened).toISOString().slice(5, 16).replace('T', ' ')}
                  </td>
                  <td className={t.side === 'buy' ? 'up' : 'down'}>{t.side}</td>
                  <td>{t.qty}</td>
                  <td>{t.entry.toFixed(d)}</td>
                  <td>
                    {t.exit.toFixed(d)}
                    {t.assumption === 'sl_first' && <span className="flag"> ⚠</span>}
                  </td>
                  <td>{t.rMultiple === null ? '—' : `${t.rMultiple.toFixed(2)}R`}</td>
                  <td className={t.realised >= 0 ? 'up' : 'down'}>{money(t.realised)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        <h3>Journal</h3>
        <label>
          What were you thinking?
          <textarea rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
        </label>
        <label>
          Tags (space or comma separated)
          <input value={tags} onChange={(e) => setTags(e.target.value)} placeholder="fomo revenge" />
        </label>
        <button className="primary" onClick={() => void save()}>
          Save note at this cursor
        </button>

        <ul className="notes">
          {data.journal.map((n) => (
            <li key={n.id}>
              <div className="muted">
                {new Date(n.cursor).toISOString().slice(5, 16).replace('T', ' ')}{' '}
                {n.tags.map((t) => (
                  <span key={t} className="badge">
                    {t}
                  </span>
                ))}
              </div>
              <div>{n.text}</div>
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}

function Stat({ label, value, tone }: { label: string; value: string; tone?: number }) {
  const cls = tone === undefined ? '' : tone > 0 ? 'up' : tone < 0 ? 'down' : ''
  return (
    <div className="stat">
      <span className="muted">{label}</span>
      <strong className={cls}>{value}</strong>
    </div>
  )
}

/**
 * A plain inline SVG. A charting library for one polyline would be a
 * dependency the project has to justify, and this does not need one.
 */
function EquityCurve({ points }: { points: EquityPoint[] }) {
  if (points.length < 2) return <p className="hint">Not enough fills to plot yet.</p>

  const w = 560
  const h = 120
  const values = points.map((p) => p.balance)
  const lo = Math.min(...values)
  const hi = Math.max(...values)
  const span = hi - lo || 1
  const path = points
    .map((p, i) => {
      const x = (i / (points.length - 1)) * w
      const y = h - ((p.balance - lo) / span) * h
      return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
  const last = values[values.length - 1]
  const first = values[0]

  return (
    <svg className="equity" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" role="img"
      aria-label={`Equity from ${first.toFixed(2)} to ${last.toFixed(2)}`}>
      <path d={path} fill="none" stroke={last >= first ? '#26a69a' : '#ef5350'} strokeWidth={1.5} />
    </svg>
  )
}
