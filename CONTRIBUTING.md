# Contributing

Thanks for looking. This is a small, deliberately narrow project, so the most
useful thing you can do before writing code is read what it refuses to be.

## Read these first

1. **[docs/invariants.md](docs/invariants.md)** — the correctness contract.
   A change that breaks an invariant is a bug even if the tests still pass.
   Code comments refer to them as `I1`–`I6`, so you can grep for the rules a
   file is upholding.
2. **[docs/adr](docs/adr)** — decisions that are already settled, and why.
   If you think one is wrong, say so in an issue before building around it.
3. **[docs/schema.md](docs/schema.md)** — the data model and on-disk layout.

## Layout

```
crates/core      model, timeframe maths, the provider interface. No I/O.
crates/data      provider adapters, Parquet + DuckDB store
crates/engine    replay cursor, fill simulation, stats, journal, export
crates/cli       fetch and inspect the cache without a UI
crates/app       Tauri shell; the IPC surface
ui/              React front end
```

Data flows one way: `data` → `engine` → `app` → `ui`. The UI is a display
client. It never filters, never decides, and is never trusted to hide anything.

## House rules

These exist because the project has a specific failure mode — quietly producing
numbers a trader might believe.

- **No file over 400 lines.** Refactor before crossing it.
- **No new dependency** without saying what it replaces and confirming its
  license is AGPL-compatible. Justify it in the PR.
- **Comments explain why, not what.** The what is the code. If a rule is
  subtle — and most of the fill logic is — say what goes wrong without it.
- **Never make data look tidier than it is.** No interpolation, no forward
  fill, no silent sorting of a user's CSV, no resolving an ambiguous fill in
  the trader's favour.
- **No new scope.** See the non-goals in
  [docs/invariants.md](docs/invariants.md#non-goals). "While I was in there"
  changes are harder to review and easier to get wrong.

## Before opening a PR

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ui && npm run build     # tsc runs in strict mode here
```

CI runs exactly these. The no-lookahead property test takes about two minutes;
that is expected.

If you changed engine or data behaviour, say in the PR which of the checks in
[the verification table](docs/invariants.md#verification) cover it — and add one
if none do.

## Reporting a bug

A bug in a backtester is often "the number was wrong", which is hard to act on
without specifics. Please include the instrument, the date range, the cursor
time, and what you expected instead. **Review → Export** produces the exact
trade log, which is usually the fastest way to show what happened.
