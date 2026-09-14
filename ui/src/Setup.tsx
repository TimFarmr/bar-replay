import { useEffect, useState } from 'react'
import { api, dateToMs, msToDate } from './api'
import { Settings } from './Settings'
import type { Coverage, Instrument, SessionSummary, View } from './types'

/**
 * The first screen. The loss function lives here: a trader with no technical
 * background should reach an honest replay in minutes, with no account, no key
 * and no terminal. So the defaults are pre-filled and the only required action
 * is pressing one button.
 */
export function Setup({ onOpen }: { onOpen: (view: View) => void }) {
  const [instruments, setInstruments] = useState<Instrument[]>([])
  const [sessions, setSessions] = useState<SessionSummary[]>([])
  const [selected, setSelected] = useState('')
  const initial = lastFullMonth()
  const [from, setFrom] = useState(initial.from)
  const [to, setTo] = useState(initial.to)
  const [balance, setBalance] = useState(10000)
  const [spread, setSpread] = useState(0)
  const [commission, setCommission] = useState(0)
  const [coverage, setCoverage] = useState<Coverage | null>(null)
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)

  const instrument = instruments.find((i) => key(i) === selected)

  useEffect(() => {
    api
      .instruments()
      .then((list) => {
        setInstruments(list)
        // Prefer an always-open market for a first run: no weekend gaps to
        // explain, and these providers are the quick ones. Chosen by property
        // rather than by naming a symbol, so the default survives the catalog
        // changing.
        const first = list.find((i) => i.market === 'continuous') ?? list[0]
        if (first !== undefined) setSelected(key(first))
      })
      .catch((e) => setError(String(e)))
    refreshSessions()
  }, [])

  useEffect(() => {
    if (instrument === undefined) return
    setCoverage(null)
    api
      .coverage(instrument.provider, instrument.symbol)
      .then((c) => {
        setCoverage(c)
        // Already downloaded? Then the useful default is what is actually
        // here, so "Start replay" works without fetching anything.
        if (c !== null) {
          setFrom(msToDate(c.from))
          setTo(msToDate(c.to + DAY_MS))
        }
      })
      .catch((e) => setError(String(e)))
  }, [selected, instruments.length])

  function refreshInstruments() {
    api.instruments().then(setInstruments).catch((e) => setError(String(e)))
  }

  function refreshSessions() {
    api.listSessions().then(setSessions).catch((e) => setError(String(e)))
  }

  async function download() {
    if (instrument === undefined) return
    setError('')
    setBusy('Downloading from the provider. This can take a while for FX.')
    try {
      const summary = await api.fetchRange(
        instrument.provider,
        instrument.symbol,
        dateToMs(from),
        dateToMs(to),
      )
      setCoverage(await api.coverage(instrument.provider, instrument.symbol))
      setBusy(
        `Downloaded ${summary.fetched} bars. ${summary.emptyDays} day(s) had no data ` +
          `at all — weekends and holidays are expected and stay gaps.`,
      )
    } catch (e) {
      setBusy('')
      setError(String(e))
    }
  }

  async function start() {
    if (instrument === undefined) return
    setError('')
    setBusy('Preparing the session…')
    try {
      onOpen(
        await api.createSession(
          instrument.provider,
          instrument.symbol,
          dateToMs(from),
          dateToMs(to),
          balance,
          spread,
          commission,
        ),
      )
    } catch (e) {
      setBusy('')
      setError(String(e))
    }
  }

  async function resume(id: string) {
    setError('')
    try {
      onOpen(await api.openSession(id))
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div className="setup">
      {settingsOpen && (
        <Settings
          instruments={instruments}
          onClose={() => setSettingsOpen(false)}
          onChanged={refreshInstruments}
        />
      )}

      <header className="setup-head">
        <button className="settings-link" onClick={() => setSettingsOpen(true)}>
          Settings
        </button>
        <h1>Bar Replay</h1>
        <p>
          Replay the market bar by bar and trade it without knowing what comes
          next. No account, no API key, no server — data comes straight from the
          provider to this machine.
        </p>
      </header>

      <section className="card">
        <h2>Start a new replay</h2>

        <label>
          Instrument
          <select value={selected} onChange={(e) => setSelected(e.target.value)}>
            {instruments.map((i) => (
              <option key={key(i)} value={key(i)}>
                {i.symbol} — {i.provider}
              </option>
            ))}
          </select>
        </label>

        <div className="row">
          <label>
            From (UTC)
            <input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
          </label>
          <label>
            To (UTC)
            <input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
          </label>
          <label>
            Starting balance
            <input
              type="number"
              min={1}
              step={100}
              value={balance}
              onChange={(e) => setBalance(Number(e.target.value))}
            />
          </label>
        </div>

        <div className="row">
          <label>
            Spread (points)
            <input
              type="number"
              min={0}
              step="any"
              value={spread}
              onChange={(e) => setSpread(Number(e.target.value))}
            />
          </label>
          <label>
            Commission per unit
            <input
              type="number"
              min={0}
              step="any"
              value={commission}
              onChange={(e) => setCommission(Number(e.target.value))}
            />
          </label>
        </div>

        <p className="coverage">
          {coverage === null
            ? 'Nothing downloaded for this instrument yet.'
            : `Cached: ${coverage.bars.toLocaleString()} one-minute bars, ` +
              `${coverage.fromLabel} .. ${coverage.toLabel} UTC.`}
        </p>
        {instrument !== undefined && (
          <p className="hint">
            Session timezone {instrument.sessionTz}.{' '}
            {instrument.spreadMode === 'historical'
              ? 'This provider publishes real bid/ask. Until tick-level spreads are wired in, the value above is used and is your assumption, not history.'
              : 'This provider publishes traded prices only, so the spread above is a value you chose — not historical fact.'}
          </p>
        )}

        <div className="row">
          <button onClick={download} disabled={busy !== ''}>
            Download this range
          </button>
          <button className="primary" onClick={start} disabled={busy !== ''}>
            Start replay
          </button>
        </div>

        {busy !== '' && <p className="busy">{busy}</p>}
        {error !== '' && <p className="error">{error}</p>}
      </section>

      <section className="card">
        <h2>Resume a session</h2>
        {sessions.length === 0 ? (
          <p className="hint">No saved sessions yet.</p>
        ) : (
          <ul className="sessions">
            {sessions.map((s) => (
              <li key={s.id}>
                <span>
                  <strong>{s.symbol}</strong> {s.rangeLabel} UTC
                </span>
                <button onClick={() => resume(s.id)}>Open</button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}

function key(i: Instrument): string {
  return `${i.provider}/${i.symbol}`
}

const DAY_MS = 86_400_000

/**
 * The last complete calendar month, in UTC.
 *
 * Computed rather than written in: a fixed date would quietly rot, and a
 * first-time user would be offered a range further out of date every year.
 * The previous month is also safely in the past, so providers have it.
 */
function lastFullMonth(): { from: string; to: string } {
  const now = new Date()
  const firstOfThis = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1)
  const firstOfLast = Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - 1, 1)
  return { from: msToDate(firstOfLast), to: msToDate(firstOfThis) }
}
