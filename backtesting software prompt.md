# Project spec — open-source bar-replay backtester

> **How to use this file:** put it in the repo root as `CLAUDE.md` and start the
> Claude Code session with: *"Read CLAUDE.md. Do the Grill-me step at the bottom
> first. Do not write code until I've answered."*

---

## 1. Mission and loss function

Build a free, open-source, local-first **manual bar-replay backtester** for
discretionary traders. A user picks an instrument and a historical date, the
chart plays forward candle by candle, and they place simulated trades without
knowing what comes next.

**Loss function — optimize every decision against this:**
*A trader with no technical background can be doing their first honest replay
session within 5 minutes of downloading the app, without an account, without a
key, and without a terminal.*

There is no revenue model. No cloud, no accounts, no telemetry, no paid tier.
When a trade-off appears between "more features" and "less friction", friction
wins.

## 2. Non-goals — say no by default

Out of scope. Do not build these, do not scaffold for them, do not add
abstractions "in case":

- Automated / programmatic strategy testing, optimizers, EAs, coded strategies
- Live broker connections or real order execution
- User accounts, cloud sync, sharing, social features, leaderboards
- Any server-side component whatsoever
- AI features of any kind in v1
- Multi-chart layouts, Monte Carlo, economic calendar, prop-firm rule presets
  (all deferred past v1 — revisit only after real users ask)

If I request one of these mid-session, remind me of this section and ask whether
it can be dropped or pushed to user-land.

## 3. Locked architecture decisions

Do not relitigate these. If you think one is wrong, say so once, then follow it.

| Decision | Choice | Reason |
|---|---|---|
| Distribution | Desktop app, signed installers for Win/macOS/Linux | zero-friction rule |
| Shell | Tauri 2 | small binaries, no bundled Chromium |
| UI | React + TypeScript | contributor familiarity |
| Storage | Local Parquet files + DuckDB for queries | fast time-series aggregation, no server |
| Charting | KLineCharts (Apache-2.0) | has drawing tools, OSS-compatible |
| License | AGPL-3.0 | prevents a closed SaaS fork |
| Data flow | Client → data provider **directly** | see below |

**The single most important architectural constraint:**
Market data must NEVER pass through any server operated by this project. Not a
proxy, not a cache, not a "convenience endpoint". The user is the data
subscriber; this app is only a display client. Any PR that routes market data
through infrastructure we control is rejected on legal grounds, not stylistic
ones.

**Never vendor the TradingView Charting Library** into this repo. Its license
forbids redistribution. Do not add it, do not reference it, do not add a
"drop your copy here" hook.

## 4. Data layer — get this right before anything else

Wrong data structures are the number one cause of throwaway rewrites. Propose
the full schema and wait for my approval before implementing.

**Provider adapters** behind one interface. Ship these:

- `dukascopy` — free FX tick/minute data, no key, no signup → **the default**
- `binance` — free full crypto kline history, no key → **the default**
- `csv` — user-supplied file import
- `databento` — BYOK, for CME futures (NQ/ES) and equities

The two free adapters must work on first launch with zero configuration. BYOK is
strictly an upgrade path, never a requirement. Keys are stored in the OS
keychain, never in a config file, never logged, never sent anywhere but the
provider's own API.

**Canonical model:** one base series per instrument at the finest resolution
available (1-minute default, ticks when the provider has them). All higher
timeframes are aggregated on demand from the base series — never stored
separately, never fetched separately.

Entities to model: `Instrument`, `CandleStore`, `Session`, `Order`, `Position`,
`Trade`, `JournalEntry`. Sessions must be resumable and must survive app
restarts and schema migrations.

**Timezone policy.** All timestamps are stored and reasoned about in UTC
internally. Session/daily/weekly candle boundaries are computed against a
per-instrument session timezone (default: exchange-local for the instrument's
primary venue — e.g. `America/New_York` for CME futures, UTC for FX/crypto),
and that timezone is always shown in the UI, never silently assumed. DST
transitions are handled through a timezone library, never manual offsets.

**Account/risk model.** One account per session: starting balance, account
currency, and an optional per-trade leverage/margin setting (off by default,
relevant only for futures/margin instruments). This is the minimum needed to
compute position sizing and R-multiples in M3/M4. No multi-account, no
portfolio-level margin netting — out of scope.

**CSV import schema.** Required columns: `timestamp, open, high, low, close`
(`volume` optional). Timestamp accepted as ISO-8601 or epoch. Rows must be
strictly ascending by timestamp; out-of-order or duplicate rows are rejected
with a report, never silently sorted or deduped.

**Cache retention.** No automatic eviction in v1. The Parquet cache is
user-managed via an explicit "clear cache for instrument" action in settings.
This is a deliberate scope cut (section 2) — document the disk-space
trade-off in the UI rather than building an eviction policy.

## 5. Correctness invariants — non-negotiable

These are what make the tool honest. A backtester that quietly leaks the future
is worse than no backtester, because it manufactures false confidence.

1. **No lookahead.** The replay cursor is a *timestamp*, never an array index.
   Data beyond the cursor must be filtered at the data-access layer, before it
   ever reaches the chart component. Never rely on the UI to hide it.
2. **Partially formed candles.** At cursor time T, the newest candle of every
   timeframe must be the *in-progress* aggregate of base bars in
   `[floor(T, tf), T]` — not the completed candle, not the previous one.
3. **Documented fill model.** When a single bar touches both stop-loss and
   take-profit, the outcome is ambiguous from OHLC alone. Resolve using
   finer-resolution data when available; otherwise assume **stop-loss first**
   (pessimistic). Show this assumption in the UI wherever it affected a trade.
   Never silently pick the favourable outcome.
   **Spread per provider:** when the provider has real bid/ask (Dukascopy),
   spread is derived from it; for trade-only providers (Binance), spread is a
   user-configured constant. The session UI always labels which mode is
   active — a trader must never mistake a synthetic spread for historical
   fact.
4. **Visible gaps, and gaps are not all the same.** Missing data is rendered
   as a gap. Never interpolate, never synthesize candles, never forward-fill
   to make a chart look tidy. Distinguish **calendar gaps** (weekend/holiday —
   expected, rendered like a normal chart's non-trading gap) from **data
   gaps** (missing/corrupt provider data during expected trading hours —
   rendered with an explicit "data missing" marker). Never conflate the two.
5. **Determinism.** The same session replayed with the same inputs produces a
   byte-identical trade log. This includes stepping backward: moving the
   cursor behind the fill time of an order rolls back that order/position
   state as if it never happened — replay time and simulation state are the
   same clock, always consistent with the cursor, never a "ghost" fill left
   behind.
6. **Sessions persist positions as-is.** Closing the app or ending a session
   with an open position never force-closes it. The position is saved exactly
   as it stood and resumes at the same cursor time on reload.

## 6. Verification — required for every milestone

Each milestone is done only when all of these pass, and you have run them and
shown me the output. "It should work" is not verification.

- Property test: for 1000 random cursor times and all supported timeframes, no
  returned candle has `timestamp > cursor`.
- Aggregation test: replay to T, read the 1h series, assert the last candle
  equals the manual aggregate of 1m bars in `[floor(T,1h), T]`.
- Fill tests: gap-open through a stop, limit order not touched, SL and TP inside
  one bar, partial close, break-even move.
- Determinism test: run the same scripted session twice, diff the trade logs.
- Step-backward rollback test: place and fill an order, step the cursor back
  past the fill time, assert the order/position state has fully reverted.
- Gap taxonomy test: a weekend/holiday gap renders differently from an
  injected missing-data gap during trading hours.
- Golden test: aggregate a known week of EURUSD 1m data to 1h and diff against a
  committed fixture.
- Manual UX check I will do myself each milestone: cold start, resize, empty
  state, no-network state, invalid key state.

## 7. Milestones

Build strictly in this order. Stop after each and wait for my review.

- **M0 — Skeleton.** Repo, license, CI, ADRs for section 3 decisions, provider
  interface with a stub, no UI.
- **M1 — Data.** Dukascopy + Binance adapters, Parquet cache, DuckDB queries,
  CLI to fetch and inspect. Verified by the aggregation and golden tests.
- **M2 — Replay.** Chart, cursor, play/pause/speed, step forward, step back,
  jump-to-date. No trading yet. Verified by the no-lookahead property test.
- **M3 — Simulation.** Market/limit/stop orders, SL/TP, partial closes,
  break-even, position sizing, spread and commission config. Verified by fill
  tests.
- **M4 — Review.** Trade log, equity curve, R-multiple stats, journal with notes
  and tags. Local export to CSV/JSON.
- **M5 — Ship.** BYOK settings UI, Databento adapter, CSV import, signed
  installers, a README a non-technical trader can follow.

## 8. Anti-slop rules

- No file over 400 lines. Refactor before you cross it.
- No new dependency without naming what it replaces and confirming its license
  is AGPL-compatible. Justify each one to me.
- Do not write code for a milestone that has not started.
- Review your own diff before telling me something is done. State explicitly
  what you checked and what you did not.
- If you are guessing at my intent, stop and ask instead of picking.
- Flag it plainly if you think the session is producing volume rather than
  progress.

## 9. Grill me first

Before writing any code, ask me 5 sharp questions about forks in the road I
probably have not thought through. Focus on data modelling, session/state
lifecycle, and where the zero-friction rule conflicts with the correctness
invariants. Do not proceed until I have answered.
