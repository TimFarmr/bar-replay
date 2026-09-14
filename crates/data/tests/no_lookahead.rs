//! The no-lookahead property test (spec §6, invariant §5.1) and the
//! partially-formed-candle test (§5.2), run against the real storage path:
//! Parquet on disk, DuckDB doing the scan and the aggregation.
//!
//! Deliberately not a unit test on the in-memory aggregator. The invariant that
//! matters is that nothing past the cursor survives the *data layer*.

use chrono_tz::Tz;
use replay_core::{Bar, Timeframe, Timestamp};
use replay_data::Store;
use std::path::PathBuf;

/// Deterministic so a failure reproduces exactly. A test that picks different
/// cursors on every run cannot be bisected.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0 >> 11
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn fixture_bars() -> Vec<Bar> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/eurusd-2024-01-01-1m.csv");
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            Bar {
                ts: Timestamp(f[0].parse().unwrap()),
                open: f[1].parse().unwrap(),
                high: f[2].parse().unwrap(),
                low: f[3].parse().unwrap(),
                close: f[4].parse().unwrap(),
                volume: f[5].parse().unwrap(),
            }
        })
        .collect()
}

fn stored(name: &str, bars: &[Bar]) -> (Store, PathBuf) {
    let dir = std::env::temp_dir().join(format!("bar-replay-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("1m.parquet");
    let store = Store::open().unwrap();
    store.upsert_bars(&path, bars).unwrap();
    (store, path)
}

#[test]
fn no_candle_at_any_timeframe_ever_reaches_past_the_cursor() {
    let bars = fixture_bars();
    let (store, path) = stored("nolookahead", &bars);
    let (first, last) = (bars[0].ts, bars.last().unwrap().ts);
    let span = (last.0 - first.0) as u64;
    let mut rng = Lcg(0x5EED);

    let mut checked = 0usize;
    for _ in 0..1000 {
        // Cursors land anywhere, including inside weekend gaps and mid-candle.
        let cursor = Timestamp(first.0 + rng.below(span) as i64);
        for tf in Timeframe::ALL {
            let candles = store.candles(&path, tf, Tz::UTC, first, cursor).unwrap();
            for c in &candles {
                assert!(
                    c.ts <= cursor,
                    "{} candle opening at {} is past cursor {}",
                    tf.label(),
                    c.ts,
                    cursor
                );
            }
            // A bucket opening at or before the cursor may still not have been
            // fully revealed, so compare against bars the cursor has reached.
            let visible: Vec<Bar> = bars.iter().copied().filter(|b| b.ts <= cursor).collect();
            let reference = replay_core::aggregate(&visible, tf, Tz::UTC, cursor);
            assert_eq!(
                candles.len(),
                reference.len(),
                "{} candle count at cursor {cursor}",
                tf.label()
            );
            if let (Some(got), Some(want)) = (candles.last(), reference.last()) {
                assert_eq!(got.high, want.high, "{} high at {cursor}", tf.label());
                assert_eq!(got.low, want.low, "{} low at {cursor}", tf.label());
                assert_eq!(got.close, want.close, "{} close at {cursor}", tf.label());
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 8000, "1000 cursors x 8 timeframes");
}

/// Spec §6: "replay to T, read the 1h series, assert the last candle equals the
/// manual aggregate of 1m bars in [floor(T,1h), T]". Computed here by hand from
/// the raw bars, not by calling the aggregator, so the two cannot agree by
/// sharing a bug.
#[test]
fn the_newest_hourly_candle_is_the_manual_aggregate_of_its_minutes_so_far() {
    let bars = fixture_bars();
    let (store, path) = stored("partial", &bars);
    let first = bars[0].ts;
    let mut rng = Lcg(0xC0FFEE);
    let mut asserted = 0;

    for _ in 0..250 {
        let cursor = bars[rng.below(bars.len() as u64) as usize].ts;
        let candles = store
            .candles(&path, Timeframe::H1, Tz::UTC, first, cursor)
            .unwrap();
        let last = candles.last().expect("cursor sits on a real bar");

        let bucket = Timeframe::H1.bucket_start(cursor, Tz::UTC);
        let window: Vec<&Bar> = bars
            .iter()
            .filter(|b| b.ts >= bucket && b.ts <= cursor)
            .collect();
        assert!(!window.is_empty());

        assert_eq!(last.ts, bucket);
        assert_eq!(last.open, window[0].open);
        assert_eq!(last.close, window.last().unwrap().close);
        assert_eq!(
            last.high,
            window.iter().map(|b| b.high).fold(f64::MIN, f64::max)
        );
        assert_eq!(
            last.low,
            window.iter().map(|b| b.low).fold(f64::MAX, f64::min)
        );

        // In progress unless the cursor's own bar covers the hour's last minute.
        let ends = Timeframe::H1.next_bucket(bucket, Tz::UTC);
        assert_eq!(
            last.complete,
            ends.0 <= cursor.0 + Timestamp::MINUTE,
            "completeness at {cursor}"
        );
        asserted += 1;
    }
    assert_eq!(asserted, 250);
}

/// Stepping the cursor forward may only ever append to, or extend, what was
/// already shown. A candle that changed retroactively would mean the earlier
/// view had leaked or the later one lost data.
#[test]
fn advancing_the_cursor_never_rewrites_an_already_closed_candle() {
    let bars = fixture_bars();
    let (store, path) = stored("monotonic", &bars);
    let first = bars[0].ts;

    let mut previous: Vec<replay_core::Candle> = Vec::new();
    for step in (0..bars.len()).step_by(37) {
        let cursor = bars[step].ts;
        let now = store
            .candles(&path, Timeframe::H1, Tz::UTC, first, cursor)
            .unwrap();
        // Every candle the previous view considered closed must be unchanged.
        for old in previous.iter().filter(|c| c.complete) {
            let found = now
                .iter()
                .find(|c| c.ts == old.ts)
                .unwrap_or_else(|| panic!("closed candle at {} vanished", old.ts));
            assert_eq!(found, old, "closed candle at {} was rewritten", old.ts);
        }
        previous = now;
    }
}
