//! Golden test: aggregate a known week of EURUSD 1-minute data to
//! 1-hour and diff against a committed fixture.
//!
//! The fixture is real Dukascopy data, so this exercises the whole stored
//! pipeline — Parquet write, DuckDB scan, SQL aggregation — not just the
//! in-memory aggregator. Regenerate with:
//!
//! ```text
//! cargo test -p replay-data --test golden -- --ignored regenerate
//! ```

use chrono_tz::Tz;
use replay_core::{Bar, Timeframe, Timestamp};
use replay_data::Store;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

const BASE: &str = "eurusd-2024-01-01-1m.csv";
const GOLDEN: &str = "eurusd-2024-01-01-1h.csv";

fn read_bars(path: &PathBuf) -> Vec<Bar> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}\nrun the `regenerate` ignored test", path.display()));
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            assert_eq!(f.len(), 6, "malformed fixture row: {line}");
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

/// Fixed precision so "diff against a committed fixture" is byte-exact
/// (ADR 0010).
fn render(ts: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> String {
    format!("{ts},{o:.5},{h:.5},{l:.5},{c:.5},{v:.2}")
}

fn temp_parquet(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bar-replay-golden-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("1m.parquet")
}

#[test]
fn golden_week_of_eurusd_aggregates_to_the_committed_hourly_series() {
    let bars = read_bars(&fixtures().join(BASE));
    assert!(bars.len() > 5000, "fixture should hold a full week");

    let store = Store::open().unwrap();
    let path = temp_parquet("hourly");
    store.upsert_bars(&path, &bars).unwrap();

    let cursor = bars.last().unwrap().ts;
    let got = store
        .candles(&path, Timeframe::H1, Tz::UTC, bars[0].ts, cursor)
        .unwrap();

    let expected = std::fs::read_to_string(fixtures().join(GOLDEN)).unwrap();
    let expected: Vec<&str> = expected.lines().skip(1).filter(|l| !l.is_empty()).collect();

    assert_eq!(got.len(), expected.len(), "hourly candle count changed");
    for (c, want) in got.iter().zip(expected) {
        assert_eq!(
            render(c.ts.0, c.open, c.high, c.low, c.close, c.volume),
            want,
            "hourly candle at {} differs from the fixture",
            c.ts
        );
    }
}

/// The stored SQL path and the dependency-free reference aggregator must agree
/// on real data, for every timeframe. If they ever diverge, one of them is
/// wrong and the golden fixture alone would not say which.
#[test]
fn sql_aggregation_matches_the_reference_aggregator_on_real_data() {
    let bars = read_bars(&fixtures().join(BASE));
    let store = Store::open().unwrap();
    let path = temp_parquet("reference");
    store.upsert_bars(&path, &bars).unwrap();

    let from = bars[0].ts;
    let cursor = bars.last().unwrap().ts;
    for tf in Timeframe::ALL {
        let sql = store.candles(&path, tf, Tz::UTC, from, cursor).unwrap();
        let reference = replay_core::aggregate(&bars, tf, Tz::UTC, cursor);
        assert_eq!(sql.len(), reference.len(), "{} candle count", tf.label());
        for (a, b) in sql.iter().zip(&reference) {
            assert_eq!(a.ts, b.ts, "{} bucket", tf.label());
            assert_eq!(a.open, b.open, "{} open at {}", tf.label(), a.ts);
            assert_eq!(a.high, b.high, "{} high at {}", tf.label(), a.ts);
            assert_eq!(a.low, b.low, "{} low at {}", tf.label(), a.ts);
            assert_eq!(a.close, b.close, "{} close at {}", tf.label(), a.ts);
            assert_eq!(a.complete, b.complete, "{} state at {}", tf.label(), a.ts);
            assert!(
                (a.volume - b.volume).abs() < 1e-6,
                "{} volume at {}: {} vs {}",
                tf.label(),
                a.ts,
                a.volume,
                b.volume
            );
        }
    }
}

/// Writes the fixtures from whatever is in the local cache. Ignored by default
/// because it needs a populated `$BAR_REPLAY_HOME`.
#[test]
#[ignore = "regenerates committed fixtures from the local cache"]
fn regenerate() {
    let root = replay_data::paths::root();
    let src = replay_data::paths::base_parquet(&root, "dukascopy", "EURUSD");
    let store = Store::open().unwrap();
    let cov = store
        .coverage(&src)
        .unwrap()
        .expect("fetch dukascopy EURUSD --from 2024-01-01 --to 2024-01-08 first");

    let bars = store.bars(&src, cov.from, cov.to).unwrap();
    let mut base = String::from("ts,open,high,low,close,volume\n");
    for b in &bars {
        base.push_str(&render(b.ts.0, b.open, b.high, b.low, b.close, b.volume));
        base.push('\n');
    }
    std::fs::create_dir_all(fixtures()).unwrap();
    std::fs::write(fixtures().join(BASE), base).unwrap();

    let candles = store
        .candles(&src, Timeframe::H1, Tz::UTC, cov.from, cov.to)
        .unwrap();
    let mut hourly = String::from("ts,open,high,low,close,volume\n");
    for c in &candles {
        hourly.push_str(&render(c.ts.0, c.open, c.high, c.low, c.close, c.volume));
        hourly.push('\n');
    }
    std::fs::write(fixtures().join(GOLDEN), hourly).unwrap();
    println!(
        "wrote {} bars and {} hourly candles",
        bars.len(),
        candles.len()
    );
}
