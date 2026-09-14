# Test fixtures

`eurusd-2024-01-01-1m.csv` is one week of real EURUSD one-minute bars
(2024-01-01 to 2024-01-07 UTC) downloaded from Dukascopy's free public
datafeed, and `eurusd-2024-01-01-1h.csv` is the hourly aggregate the code is
expected to produce from it.

The spec (§6) asks for a golden test against "a known week of EURUSD 1m data",
and real data is what makes it meaningful: it contains a weekend gap, a
holiday, and minutes Dukascopy pads with flat zero-volume records, all of which
the aggregation has to handle correctly. Synthetic data would exercise none of
that.

## Regenerating

```sh
bar-replay fetch dukascopy EURUSD --from 2024-01-01 --to 2024-01-08
cargo test -p replay-data --test golden -- --ignored regenerate
```

## A note on provenance

This is a small excerpt of factual market data, committed so the test suite is
hermetic and runs offline in CI. It is not a redistribution of Dukascopy's
feed, and the app itself never routes market data through any infrastructure
this project operates (ADR 0007) — users download it themselves, directly.

If that judgement is ever disputed, the fix is cheap: the golden test can run
against generated data instead, at the cost of no longer exercising real
weekend gaps and real provider padding. Worth deciding deliberately rather than
by accident.
