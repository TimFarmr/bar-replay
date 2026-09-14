# ADR 0012: A session pins the candle data it was started with

Status: accepted (grill-me round 1, 2026-09-06)

## Context
Providers silently correct and backfill history. A session paused for weeks
must not resume against different bars than the ones its trades were logged
against.

## Decision
At creation, the session copies its base-series slice into its own directory.
Ticks fetched lazily during the session (ADR 0013) are stored there too.
Extending the range appends; nothing already pinned is ever rewritten.

## Consequences
Sessions are self-contained; the instrument cache is disposable. Disk cost is
a few MB per session.
