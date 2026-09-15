# ADR 0009: The trade ledger is the source of truth; positions are derived

Status: accepted (grill-me round 1, 2026-09-06)

## Decision
`Trade` is an append-only ledger row emitted on every fill (entry, partial
close, full close, SL/TP hit). `Position` is computed by folding the ledger,
never stored as independently mutable state.

## Consequences
Determinism (invariant I5) is checkable: two replays either produce the same
ledger or they do not. There is no second copy of state to drift.
