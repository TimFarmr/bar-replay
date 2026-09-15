# The correctness contract

A backtester that quietly leaks the future is worse than no backtester, because
it manufactures confidence its user has not earned. Everything in this document
exists to stop that happening.

These are not style preferences. They are the reason the project is worth using,
and a change that breaks one is a bug even if every test still passes. Code
comments refer to them by their short names — `I1`, `I4` and so on — so you can
`grep` for the rules a given file is upholding.

---

## The invariants

### I1 — No lookahead

The replay cursor is a **timestamp**, never an array index. Data beyond the
cursor is filtered **at the data-access layer**, before it can reach the chart
component. The UI is never trusted to hide anything.

In practice: the `ts <= cursor` predicate lives inside the SQL scan in
`crates/data/src/store.rs`. Nothing above that layer ever holds a bar the user
should not be able to see.

### I2 — Partially formed candles

At cursor time `T`, the newest candle of every timeframe is the *in-progress*
aggregate of the base bars in `[floor(T, tf), T]`. Not the completed candle, not
the previous one.

A trader stepping through a 1-minute replay on an hourly chart must watch the
hourly candle form, exactly as they would live.

### I3 — A documented fill model, never a flattering one

When a single bar touches both stop-loss and take-profit, the outcome is
genuinely ambiguous from OHLC alone. Resolve it with finer-resolution data when
that data is available; otherwise **assume stop-loss first** — the pessimistic
answer — and mark the trade so the UI can say the result rested on an
assumption. Never silently pick the favourable outcome.

**Spread follows the provider.** Where the provider publishes real bid/ask, a
historical spread can be derived. Where it publishes trade prints only, the
spread is a value the user chose. The session UI always states which mode is
active: a trader must never mistake a synthetic spread for historical fact.

### I4 — Visible gaps, and gaps are not all the same

Missing data is rendered as a gap. Never interpolate, never synthesise candles,
never forward-fill to make a chart look tidy.

Two kinds, never conflated:

- **Calendar gap** — a weekend or holiday. Expected, and drawn the way any
  chart skips non-trading time.
- **Data gap** — missing or corrupt provider data during hours the market was
  open. Rendered with an explicit "data missing" marker.

A broken download must never be able to masquerade as a quiet Sunday.

### I5 — Determinism, including backwards

The same session replayed with the same inputs produces a byte-identical trade
log.

This includes stepping backward: moving the cursor behind the fill time of an
order rolls that order and position back *as if it had never happened*. Replay
time and simulation state are the same clock. No ghost fills.

The implementation makes this structural rather than aspirational — trading
state is a pure function of the event log and the cursor, so rollback is not
code that can rot. See [ADR 0015](adr/0015-simulation-is-recomputed-from-the-log.md).

### I6 — Sessions persist positions as they stood

Closing the app, or ending a session with an open position, never force-closes
it. The position is saved exactly as it was and resumes at the same cursor time
on reload.

---

## Data policy

**Market data must never pass through any server operated by this project.**
Not a proxy, not a cache, not a "convenience endpoint". The user is the data
subscriber; this app is only a display client. Any change that routes market
data through infrastructure we control is rejected on legal grounds, not
stylistic ones. See [ADR 0007](adr/0007-direct-provider-data-flow.md).

**Never vendor the TradingView Charting Library.** Its license forbids
redistribution. Do not add it, do not reference it, do not add a "drop your copy
here" hook.

**Canonical model.** One base series per instrument at the finest resolution
available (1-minute by default). Every higher timeframe is aggregated on demand
from that base series — never stored separately, never fetched separately.

**Timezones.** All timestamps are stored and reasoned about in UTC internally.
Session, daily and weekly candle boundaries are computed against a per-instrument
session timezone, and that timezone is always shown in the UI, never silently
assumed. DST is handled through a timezone library, never manual offsets.

**Keys.** BYOK credentials live in the OS keychain. Never in a config file,
never in a log, never sent anywhere but the provider's own API.

**CSV import.** Required columns `timestamp, open, high, low, close`; `volume`
optional. Timestamps as ISO-8601 or epoch. Rows must be strictly ascending —
out-of-order or duplicate rows are **rejected with a report**, never silently
sorted or deduped. Sorting a user's file would fabricate a price history that
never happened.

**Cache retention.** No automatic eviction. The cache is user-managed through an
explicit "clear downloads" action. A deliberate scope cut: the disk-space
trade-off is documented in the UI rather than solved with an eviction policy.

---

## Verification

A change to the engine or the data layer is not done until these pass. "It
should work" is not verification.

| Check | What it proves |
|---|---|
| No-lookahead property test | 1000 random cursor times × every timeframe: no returned candle is past the cursor (I1) |
| Aggregation test | The newest candle equals the manual aggregate of its base bars (I2) |
| Fill tests | Gap-open through a stop, limit never touched, SL and TP inside one bar, partial close, break-even (I3) |
| Determinism test | The same scripted session twice, trade logs diffed (I5) |
| Step-backward rollback test | Fill an order, step back past it, assert the state fully reverted (I5) |
| Gap taxonomy test | A weekend renders differently from an injected data gap (I4) |
| Golden test | A known week of real EURUSD 1m data aggregated to 1h, diffed against a committed fixture |

Run them with `cargo test --workspace`. The no-lookahead test takes about two
minutes on its own; that is expected, it runs eight thousand queries.

---

## Non-goals

Deliberately out of scope. Not "later" — no scaffolding, no abstractions "in
case":

- Automated or programmatic strategy testing, optimisers, coded strategies
- Live broker connections or real order execution
- User accounts, cloud sync, sharing, social features, leaderboards
- Any server-side component whatsoever
- AI features
- Multi-chart layouts, Monte Carlo, economic calendar, prop-firm rule presets

The project optimises for one thing: **a trader with no technical background
doing their first honest replay within five minutes of downloading it — without
an account, without a key, and without a terminal.** Where more features and
less friction have been in conflict, friction has won.
