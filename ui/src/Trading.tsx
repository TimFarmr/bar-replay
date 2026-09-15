import { useState } from 'react'
import { api } from './api'
import type { Instrument, OrderKind, Side, Trading as TradingState, View } from './types'

/**
 * The order ticket, the open position and the ledger.
 *
 * Everything here is a view over state the Rust engine recomputed from the
 * event log; nothing is tracked in React. That is deliberate — a second copy of
 * position state in the UI is exactly how a "ghost fill" survives a step
 * backwards (invariant I5).
 */
export function Trading({
  trading,
  instrument,
  onChange,
  onError,
}: {
  trading: TradingState
  instrument: Instrument
  onChange: (v: View) => void
  onError: (message: string) => void
}) {
  const [kind, setKind] = useState<OrderKind>('market')
  const [qty, setQty] = useState(1)
  const [price, setPrice] = useState('')
  const [sl, setSl] = useState('')
  const [tp, setTp] = useState('')

  const d = instrument.priceDecimals
  const px = (v: number) => v.toFixed(d)
  const money = (v: number) => v.toFixed(2)

  async function guard(work: () => Promise<View>) {
    try {
      onChange(await work())
    } catch (e) {
      onError(String(e))
    }
  }

  const num = (s: string): number | null => {
    const t = s.trim()
    if (t === '') return null
    const v = Number(t)
    return Number.isFinite(v) ? v : null
  }

  function send(side: Side) {
    if (kind !== 'market' && num(price) === null) {
      onError('A limit or stop order needs a price.')
      return
    }
    void guard(() => api.placeOrder(side, kind, qty, num(price), num(sl), num(tp)))
  }

  const pos = trading.position
  const open = trading.equity - trading.balance

  return (
    <aside className="trading">
      <div className="account">
        <div>
          <span className="muted">Balance</span>
          <strong>{money(trading.balance)}</strong>
        </div>
        <div>
          <span className="muted">Equity</span>
          <strong className={open > 0 ? 'up' : open < 0 ? 'down' : ''}>
            {money(trading.equity)}
          </strong>
        </div>
      </div>

      <section className="ticket">
        <h3>New order</h3>
        <div className="seg">
          {(['market', 'limit', 'stop'] as const).map((k) => (
            <button
              key={k}
              className={k === kind ? 'tf on' : 'tf'}
              onClick={() => setKind(k)}
            >
              {k}
            </button>
          ))}
        </div>

        <label>
          Quantity
          <input
            type="number"
            min={0}
            step="any"
            value={qty}
            onChange={(e) => setQty(Number(e.target.value))}
          />
        </label>
        {kind !== 'market' && (
          <label>
            Trigger price
            <input value={price} onChange={(e) => setPrice(e.target.value)} placeholder="—" />
          </label>
        )}
        <div className="row">
          <label>
            Stop loss
            <input value={sl} onChange={(e) => setSl(e.target.value)} placeholder="none" />
          </label>
          <label>
            Take profit
            <input value={tp} onChange={(e) => setTp(e.target.value)} placeholder="none" />
          </label>
        </div>

        <div className="row">
          <button className="buy" onClick={() => send('buy')}>
            Buy
          </button>
          <button className="sell" onClick={() => send('sell')}>
            Sell
          </button>
        </div>
      </section>

      <section>
        <h3>Position</h3>
        {pos === null ? (
          <p className="hint">Flat.</p>
        ) : (
          <div className="position">
            <div className={pos.side === 'buy' ? 'up' : 'down'}>
              {pos.side === 'buy' ? 'LONG' : 'SHORT'} {pos.qty} @ {px(pos.avgEntry)}
            </div>
            <div className={pos.unrealised >= 0 ? 'up' : 'down'}>
              open P&amp;L {money(pos.unrealised)}
            </div>
            <div className="muted">
              SL {pos.sl === null ? '—' : px(pos.sl)} · TP{' '}
              {pos.tp === null ? '—' : px(pos.tp)}
            </div>
            <div className="row">
              <button onClick={() => void guard(() => api.closePosition(null))}>Close</button>
              <button onClick={() => void guard(() => api.closePosition(pos.qty / 2))}>
                Close half
              </button>
              <button
                title="Move the stop to the entry price."
                onClick={() => void guard(() => api.modify(null, pos.avgEntry, pos.tp))}
              >
                Break-even
              </button>
            </div>
          </div>
        )}
      </section>

      {trading.working.length > 0 && (
        <section>
          <h3>Working orders</h3>
          <ul className="orders">
            {trading.working.map((o) => (
              <li key={o.id}>
                <span>
                  {o.side} {o.kind} {o.qty} @ {o.price === null ? '—' : px(o.price)}
                </span>
                <button onClick={() => void guard(() => api.cancelOrder(o.id))}>cancel</button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="ledger">
        <h3>Trades</h3>
        {trading.trades.length === 0 ? (
          <p className="hint">No fills yet.</p>
        ) : (
          <table>
            <tbody>
              {[...trading.trades].reverse().map((t) => (
                <tr key={t.seq}>
                  <td className="muted">{t.when.slice(5, 16)}</td>
                  <td className={t.side === 'buy' ? 'up' : 'down'}>{t.side}</td>
                  <td>{t.qty}</td>
                  <td>{px(t.price)}</td>
                  <td>
                    {t.reason}
                    {/* Invariant I3: say so wherever the assumption affected a trade. */}
                    {t.assumption === 'sl_first' && (
                      <span
                        className="flag"
                        title="This bar touched both the stop and the target. With no finer data the stop was assumed to come first — the pessimistic outcome."
                      >
                        {' '}
                        ⚠assumed
                      </span>
                    )}
                  </td>
                  <td className={t.realised >= 0 ? 'up' : 'down'}>{money(t.realised)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </aside>
  )
}
