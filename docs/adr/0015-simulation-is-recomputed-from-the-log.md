# ADR 0015: Trading state is recomputed from the event log on every cursor move

Status: accepted (M3, 2026-09-14)

## Context
Invariant I5 requires that moving the cursor back behind a fill rolls that fill
back "as if it never happened", with no ghost state left behind. The obvious
implementation — mutable position state plus an undo path — means writing and
maintaining a second code path that is exercised rarely and wrong quietly.

## Decision
`sim::simulate` is a pure function of `(config, bars, events, cursor)`. Nothing
is mutated across cursor moves. Stepping backward is not an undo operation; it
is the same computation over fewer bars.

## Consequences
Rollback cannot rot, because there is no rollback code. Determinism (ADR 0010)
falls out for free: the same log through the same engine is the same call.

The cost is recomputation on every move, which is why the session's bars and
event log are held in memory rather than re-read from disk. A month of
one-minute bars is ~44k iterations per step, which is immeasurable next to the
IPC round trip. If a multi-year session ever makes this hurt, the fix is to
cache state at the last cursor and advance incrementally when stepping
forward — but not before it is measured.
