# ADR 0005: KLineCharts for charting

Status: accepted (locked by docs/adr)

## Context
The chart needs candlesticks and drawing tools. The TradingView Charting
Library's license forbids redistribution, so it can never be vendored,
referenced, or given a "drop your copy here" hook.

## Decision
KLineCharts (Apache-2.0, AGPL-compatible).

## Consequences
The chart component receives only data at or before the cursor. Filtering
happens in the data layer, never in the chart (invariant I1).
