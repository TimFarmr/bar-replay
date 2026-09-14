import { useEffect, useState } from 'react'
import { api } from './api'
import type { Instrument, ProviderInfo } from './types'

/**
 * Bring-your-own-key, CSV import and cache management.
 *
 * Keys are written to the operating system's keychain and never come back
 * here: this screen can only ever learn *whether* one is set.
 */
export function Settings({
  instruments,
  onClose,
  onChanged,
}: {
  instruments: Instrument[]
  onClose: () => void
  onChanged: () => void
}) {
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [keys, setKeys] = useState<Record<string, string>>({})
  const [path, setPath] = useState('')
  const [symbol, setSymbol] = useState('')
  const [decimals, setDecimals] = useState(2)
  const [note, setNote] = useState('')
  const [error, setError] = useState('')

  function loadProviders() {
    api.providers().then(setProviders).catch((e) => setError(String(e)))
  }

  useEffect(loadProviders, [])

  async function saveKey(p: ProviderInfo) {
    setError('')
    try {
      await api.setApiKey(p.id, keys[p.id] ?? '')
      setKeys((k) => ({ ...k, [p.id]: '' }))
      loadProviders()
      setNote(`Key saved to your system keychain for ${p.id}.`)
    } catch (e) {
      setError(String(e))
    }
  }

  async function removeKey(p: ProviderInfo) {
    setError('')
    try {
      await api.clearApiKey(p.id)
      loadProviders()
      setNote(`Key for ${p.id} removed from the keychain.`)
    } catch (e) {
      setError(String(e))
    }
  }

  async function chooseFile() {
    setError('')
    try {
      const chosen = await api.pickCsv()
      if (chosen === null) return
      setPath(chosen)
      if (symbol === '') {
        const base = chosen.split(/[/\\]/).pop() ?? ''
        setSymbol(base.replace(/\.[^.]+$/, '').toUpperCase().slice(0, 20))
      }
    } catch (e) {
      setError(String(e))
    }
  }

  async function doImport() {
    setError('')
    setNote('')
    try {
      const cov = await api.importCsv(path, symbol, decimals)
      setNote(
        `Imported ${cov.bars.toLocaleString()} candles, ${cov.fromLabel} .. ${cov.toLabel} UTC.`,
      )
      onChanged()
    } catch (e) {
      setError(String(e))
    }
  }

  async function forget(i: Instrument) {
    setError('')
    try {
      await api.clearCache(i.provider, i.symbol)
      setNote(
        `Cleared downloaded candles for ${i.symbol}. Saved sessions keep their own copy.`,
      )
      onChanged()
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div className="drawer">
      <header className="bar">
        <strong>Settings</strong>
        <span className="spacer" />
        <button onClick={onClose}>close</button>
      </header>

      <div className="drawer-body">
        {note !== '' && <p className="hint">{note}</p>}
        {error !== '' && <p className="error">{error}</p>}

        <h3>Import your own CSV</h3>
        <p className="hint">
          Needs the columns <code>timestamp, open, high, low, close</code>, and
          optionally <code>volume</code>. Rows must be in time order with no
          duplicates — a file that is out of order is rejected with the line
          number rather than quietly sorted, because sorting it would invent a
          price history that never happened.
        </p>
        <div className="row">
          <button onClick={() => void chooseFile()}>Choose file…</button>
          <label>
            Name it
            <input
              value={symbol}
              onChange={(e) => setSymbol(e.target.value)}
              placeholder="MYDATA"
            />
          </label>
          <label>
            Decimal places
            <input
              type="number"
              min={0}
              max={10}
              value={decimals}
              onChange={(e) => setDecimals(Number(e.target.value))}
            />
          </label>
          <button
            className="primary"
            disabled={path === '' || symbol === ''}
            onClick={() => void doImport()}
          >
            Import
          </button>
        </div>
        {path !== '' && <p className="hint">{path}</p>}

        <h3>API keys (optional)</h3>
        <p className="hint">
          The free providers need no key at all. A key is only needed for the
          paid providers below. Keys are stored in your operating system&apos;s
          keychain — never in a file, never in a log, and never sent anywhere
          except that provider&apos;s own API.
        </p>
        {providers.filter((p) => p.needsKey).length === 0 ? (
          <p className="hint">No provider here needs a key.</p>
        ) : (
          providers
            .filter((p) => p.needsKey)
            .map((p) => (
              <div key={p.id} className="keyrow">
                <div className="muted">{p.label}</div>
                {p.hasKey ? (
                  <div className="row">
                    <span className="badge good">a key is stored</span>
                    <button onClick={() => void removeKey(p)}>Remove it</button>
                  </div>
                ) : (
                  <div className="row">
                    <label>
                      Key
                      <input
                        type="password"
                        value={keys[p.id] ?? ''}
                        onChange={(e) =>
                          setKeys((k) => ({ ...k, [p.id]: e.target.value }))
                        }
                      />
                    </label>
                    <button
                      className="primary"
                      disabled={(keys[p.id] ?? '').trim() === ''}
                      onClick={() => void saveKey(p)}
                    >
                      Save
                    </button>
                  </div>
                )}
              </div>
            ))
        )}

        <h3>Downloaded data</h3>
        <p className="hint">
          Nothing is deleted automatically, so candles you download stay until
          you say otherwise. Clearing one does not affect saved sessions: each
          keeps its own copy of the candles it started with.
        </p>
        <ul className="sessions">
          {instruments.map((i) => (
            <li key={`${i.provider}/${i.symbol}`}>
              <span>
                <strong>{i.symbol}</strong>{' '}
                <span className="muted">{i.provider}</span>
              </span>
              <button onClick={() => void forget(i)}>Clear downloads</button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}
