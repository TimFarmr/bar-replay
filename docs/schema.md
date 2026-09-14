# Data schema — implemented (spec §4)

Decisions it rests on: ADR 0008–0015. Where the built code differs from the
original proposal, the difference is noted inline and marked **changed**.

## Cursor semantics (used everywhere below)

- All timestamps are UTC milliseconds (`i64`). A bar's timestamp is its **open time**.
- The cursor `C` is the open time of the newest visible base bar. A base bar is
  visible iff `ts <= C`. `C` only ever equals the `ts` of an existing base bar and
  advances to the next existing bar, so gaps are skipped, never filled (§5.4).
- The newest candle of timeframe `tf` at cursor `C` is the aggregate of base bars
  with `floor(C, tf) <= ts <= C` (§5.2). It is in progress unless
  `C == floor(C, tf) + tf - 1min`.
- The visibility filter `ts <= C` is applied in the DuckDB query, before any data
  reaches the UI (§5.1). Nothing above the data layer ever holds bars past `C`.

Inherent to 1-minute replay: stepping to bar `C` reveals that whole minute at
once. That is the resolution floor, not a leak; it applies equally to every bar.

## On-disk layout

```
<app-data>/
  instruments/<provider>/<symbol>/
    instrument.json           Instrument
    1m.parquet                base series (download cache, disposable)
    ticks/<yyyy-mm-dd-hh>.parquet
  sessions/<id>/
    session.json              Session header, immutable after creation
    1m.parquet                pinned copy of the base slice (ADR 0012)
    ticks/<yyyy-mm-dd-hh>.parquet   pinned ticks fetched during the session
    events.jsonl              append-only input log (ADR 0010, 0011)
```

`<id>` is the creation time in unix ms.
<!-- ponytail: ms-timestamp ids collide only if two sessions are created in the
same ms on one machine, impossible from a manual UI; switch to a random suffix
if sessions are ever created programmatically. -->

## Entities

### Instrument — `instrument.json`

| field | type | used from | notes |
|---|---|---|---|
| provider | string | M1 | `dukascopy` `binance` `csv` `databento` |
| symbol | string | M1 | provider-native, e.g. `EURUSD`, `BTCUSDT` |
| price_decimals | int | M2 | display and export precision (ADR 0010) |
| point | float | M3 | smallest price increment |
| multiplier | float | M3 | P&L per 1 unit per 1.0 price move: 1 for FX/crypto, 20 for NQ, 50 for ES |
| quote_currency | string | M3 | display only |

### CandleStore — Parquet files

`1m.parquet`: `ts BIGINT, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE`,
sorted by `ts`, unique `ts`. Missing minutes are absent rows.

`ticks/<hour>.parquet`: `ts BIGINT, bid DOUBLE, ask DOUBLE`, sorted by `ts`. Trade-print
providers write `bid == ask`.

Higher timeframes are one query, never a file:

```sql
SELECT (ts // $tf) * $tf AS ts,
       first(open ORDER BY ts) AS open, max(high) AS high, min(low) AS low,
       last(close ORDER BY ts) AS close, sum(volume) AS volume
FROM '1m.parquet'
WHERE ts >= $from AND ts <= $cursor        -- §5.1 lives here
GROUP BY 1 ORDER BY 1
```

### Session — `session.json`

| field | type | notes |
|---|---|---|
| schema_version | int | migrations are one function per bump (ADR 0011) |
| created_ms | int | wall clock, informational only, never an engine input |
| provider, symbol | string | one instrument per session (ADR 0008) |
| range_from, range_to | int | pinned slice; `range_to` may grow, never shrink |
| balance | float | starting balance |
| spread_points | float | applied to market and stop fills (M3) |
| commission_per_unit | float | (M3) |

### Events — `events.jsonl`, one JSON object per line

Every line has `seq` (1-based, dense) and `cursor` (the `C` at which the user acted).
All ids below are the `seq` of the event that created the thing.

| type | payload | notes |
|---|---|---|
| cursor_set | `to` | manual step, jump, or pause after play; resume reads the last one |
| order_place | `side` `kind` `qty` `price?` `sl?` `tp?` | `kind` ∈ market, limit, stop; `price` required unless market |
| order_modify | `order?` `sl?` `tp?` | **changed**: sets stop and target outright, and `order` of null targets the open position. Break-even is a modify with `sl = entry`. Re-pricing a resting order is cancel-and-replace, which needs no extra event type. |
| order_cancel | `order` | |
| position_close | `qty` | market close, full or partial |
| note_set | `note` `text` `tags` `trade?` | journal entry; last write per `note` id wins |

**changed — cursor events coalesce.** A run of consecutive `cursor_set` events
collapses to the latest one. The engine walks every bar between two cursors
regardless of how the user got there, so the intermediate positions carry no
information, and writing one line per bar would turn a played-through session
into tens of thousands of lines that say nothing. The log is rewritten through
a temporary file and renamed, so a crash mid-write leaves the previous log
intact.

The engine processes every base bar between the previous and the new cursor, in
order, whether the user stepped once or jumped a week. That is why steps need not
all be logged: fills depend only on orders and bars.

### Order, Position — derived, never stored

Folded from events by the engine on load. `Position` = `side, qty, avg_entry, sl, tp,
opened_cursor`, or none.

### Trade — the ledger, derived, exportable

One row per fill, in fill order:

| field | notes |
|---|---|
| seq | ledger position |
| cursor | `ts` of the bar in which the fill happened |
| order | id of the originating order |
| side, qty, price | |
| role | `entry` or `exit` |
| reason | `market` `limit` `stop` `sl` `tp` `close` |
| assumption | `none`, `sl_first`, or `resolved_by_ticks` (§5.3, ADR 0013); shown in the UI |
| risk_per_unit | **changed**: distance from entry to the stop set at entry. Recorded on entry rows because an R-multiple cannot be reconstructed later once the stop has been moved. `null` when the trade was taken without a stop. |
| commission | |

Round-trip trades for R-multiples (M4) pair entries with their exits; they are a
view over this table, not a second table.

### JournalEntry — derived from `note_set` events

`id, cursor, trade?, text, tags[]`. Edits are new events with the same `id`.

## Numbers and determinism

Prices, quantities and balances are `f64`. OHLC aggregation is min/max/first/last
(exact); volume sums are done in `ts` order (deterministic). Exports print prices
with `price_decimals` and money with 2 decimals, so byte-identical is well defined
(ADR 0010).

## Migration policy

`schema_version` lives in `session.json`. Loading a session runs `migrate(v → v+1)`
steps until current. Adding an optional field needs no bump. The instrument cache
has no version: on incompatible change it is deleted and refetched.
