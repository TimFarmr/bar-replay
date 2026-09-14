# ADR 0008: A session is one instrument over one date range

Status: accepted (grill-me round 1, 2026-09-06)

## Decision
`Session` binds exactly one instrument and one contiguous date range.
Switching instruments starts a new session.

## Consequences
Session and candle data are 1:1, so a session can pin its data (ADR 0012).
Orders and positions carry no instrument field. Multi-instrument replay is
deferred with multi-chart layouts (spec §2).
