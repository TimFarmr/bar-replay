# ADR 0010: Determinism means replaying a cursor-keyed input log

Status: accepted (grill-me round 1, 2026-09-06)

## Context
A human never clicks at the same wall-clock millisecond twice, so "the same
session twice" cannot mean free-play.

## Decision
Every user action (place/modify/cancel order, close, cursor move, note) is
logged keyed to the **cursor timestamp**, never wall-clock time. Determinism
means: the same input log through the same engine version yields a
byte-identical trade ledger. The §6 determinism test is a scripted session.

## Consequences
Wall-clock time appears nowhere in the engine's inputs. Exports format numbers
with fixed precision so "byte-identical" is well defined.
