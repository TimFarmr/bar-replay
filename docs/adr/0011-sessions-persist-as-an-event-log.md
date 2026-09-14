# ADR 0011: Sessions persist as an append-only event log

Status: accepted (grill-me round 1, 2026-09-06)

## Decision
A session is a header plus an append-only event log (the input log of
ADR 0010). Each event is durable the moment it happens. Resuming replays
the log.

## Consequences
One mechanism serves persistence, crash-safety, resumability and the
determinism test. Schema migrations are a per-version function over the
header and events; adding fields is backward-compatible.
