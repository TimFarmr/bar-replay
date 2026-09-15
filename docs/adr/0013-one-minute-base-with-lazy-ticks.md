# ADR 0013: 1-minute base series, ticks fetched lazily for fill resolution

Status: accepted (grill-me round 1, 2026-09-06)

## Context
Dukascopy has tick data back to ~2003 and Binance trade data back to ~2017,
so finer data is the rule, not the exception. But a year of ticks is
gigabytes, which breaks the 5-minute rule for a first session.

## Decision
The base series is 1-minute bars. When a bar touches both stop-loss and
take-profit of an order, the engine fetches ticks for that bar's hour and
walks them to find which was hit first. If the provider has no ticks or the
fetch fails, stop-loss is assumed first and the trade is flagged in the UI
(invariant I3).

## Consequences
First session is fast. Every trade that relied on the pessimistic assumption
carries `assumption: "sl_first"` in the ledger.
