//! Sessions survive app restarts (spec §4) and are pinned to the data they
//! were started with (ADR 0012).

use replay_core::{Instrument, Market, SpreadMode, Timestamp};
use replay_data::{paths, Store};
use replay_engine::session::{Account, Event, Session};
use replay_engine::Replay;
use std::path::PathBuf;

fn eurusd() -> Instrument {
    Instrument {
        provider: "dukascopy".into(),
        symbol: "EURUSD".into(),
        price_decimals: 5,
        point: 1e-5,
        multiplier: 1.0,
        quote_currency: "USD".into(),
        session_tz: chrono_tz::Tz::UTC,
        spread_mode: SpreadMode::Historical,
        market: Market::FxWeek,
    }
}

fn fixture_bars() -> Vec<replay_core::Bar> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/tests/fixtures/eurusd-2024-01-01-1m.csv");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            replay_core::Bar {
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

/// A throwaway app-data root seeded with a cached EURUSD week.
fn seeded_root(name: &str) -> (PathBuf, Vec<replay_core::Bar>) {
    let root = std::env::temp_dir().join(format!("bar-replay-session-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    let bars = fixture_bars();
    let store = Store::open().unwrap();
    store
        .upsert_bars(&paths::base_parquet(&root, "dukascopy", "EURUSD"), &bars)
        .unwrap();
    (root, bars)
}

#[test]
fn a_session_reopens_at_the_cursor_it_was_left_at() {
    let (root, bars) = seeded_root("resume");
    let session = Session::create(
        &root,
        &eurusd(),
        bars[0].ts,
        Timestamp(bars.last().unwrap().ts.0 + Timestamp::MINUTE),
        Account {
            balance: 10_000.0,
            spread_points: 0.0,
            commission_per_unit: 0.0,
        },
        1_700_000_000_000,
    )
    .unwrap();

    // A fresh session has not moved.
    assert_eq!(session.resume_cursor(&root).unwrap(), None);

    let store = Store::open().unwrap();
    let pinned = store
        .bars(
            &session.bars_path(&root),
            session.range_from,
            session.range_to,
        )
        .unwrap();
    let mut replay = Replay::new(pinned.iter().map(|b| b.ts).collect()).unwrap();

    replay.advance(500);
    let mut log = Vec::new();
    session
        .record(
            &root,
            &mut log,
            replay.cursor(),
            Event::CursorSet {
                to: replay.cursor(),
            },
        )
        .unwrap();
    let left_at = replay.cursor();

    // Restart: nothing in memory survives.
    let reopened = Session::load(&root, &session.id).unwrap();
    assert_eq!(reopened.resume_cursor(&root).unwrap(), Some(left_at));
    assert_eq!(reopened.balance, 10_000.0);
    assert_eq!(reopened.instrument.symbol, "EURUSD");
    assert_eq!(reopened.instrument.market, Market::FxWeek);
}

#[test]
fn events_are_numbered_densely_and_kept_in_order() {
    let (root, bars) = seeded_root("events");
    let session = Session::create(
        &root,
        &eurusd(),
        bars[0].ts,
        Timestamp(bars.last().unwrap().ts.0 + Timestamp::MINUTE),
        Account {
            balance: 5_000.0,
            spread_points: 0.0,
            commission_per_unit: 0.0,
        },
        1_700_000_000_001,
    )
    .unwrap();

    // Five cursor moves in a row collapse to the latest: replaying walks every
    // bar between two cursors anyway, so the intermediate ones say nothing.
    let mut log = Vec::new();
    for i in 1..=5 {
        let to = bars[i * 10].ts;
        session
            .record(&root, &mut log, to, Event::CursorSet { to })
            .unwrap();
    }
    assert_eq!(log.len(), 1, "a run of cursor moves is one entry");
    assert_eq!(session.events(&root).unwrap().len(), 1);
    assert_eq!(session.resume_cursor(&root).unwrap(), Some(bars[50].ts));

    // An order between two moves stops them collapsing across it.
    session
        .record(
            &root,
            &mut log,
            bars[60].ts,
            Event::PositionClose { qty: None },
        )
        .unwrap();
    session
        .record(
            &root,
            &mut log,
            bars[70].ts,
            Event::CursorSet { to: bars[70].ts },
        )
        .unwrap();
    let events = session.events(&root).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "seq stays dense"
    );
}

/// ADR 0012: the instrument cache is disposable, a session's copy is not.
#[test]
fn a_session_still_replays_after_its_source_cache_is_deleted() {
    let (root, bars) = seeded_root("pinned");
    let session = Session::create(
        &root,
        &eurusd(),
        bars[0].ts,
        Timestamp(bars.last().unwrap().ts.0 + Timestamp::MINUTE),
        Account {
            balance: 1_000.0,
            spread_points: 0.0,
            commission_per_unit: 0.0,
        },
        1_700_000_000_002,
    )
    .unwrap();

    // Simulate "clear cache for instrument" from settings.
    std::fs::remove_dir_all(root.join("instruments")).unwrap();

    let store = Store::open().unwrap();
    let pinned = store
        .bars(
            &session.bars_path(&root),
            session.range_from,
            session.range_to,
        )
        .unwrap();
    assert_eq!(
        pinned.len(),
        bars.len(),
        "the session keeps its own bars regardless of the cache"
    );
    assert!(Replay::new(pinned.iter().map(|b| b.ts).collect()).is_some());
}

#[test]
fn a_session_over_a_range_with_no_cached_bars_is_refused_not_opened_empty() {
    let (root, _) = seeded_root("empty");
    let err = Session::create(
        &root,
        &eurusd(),
        Timestamp(1_900_000_000_000),
        Timestamp(1_900_086_400_000),
        Account {
            balance: 1_000.0,
            spread_points: 0.0,
            commission_per_unit: 0.0,
        },
        1_700_000_000_003,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("no cached bars"),
        "unexpected error: {err}"
    );
}

#[test]
fn sessions_are_listed_newest_first() {
    let (root, bars) = seeded_root("list");
    let from = bars[0].ts;
    let to = Timestamp(bars.last().unwrap().ts.0 + Timestamp::MINUTE);
    for created in [1_700_000_000_100i64, 1_700_000_000_300, 1_700_000_000_200] {
        Session::create(
            &root,
            &eurusd(),
            from,
            to,
            Account {
                balance: 1_000.0,
                spread_points: 0.0,
                commission_per_unit: 0.0,
            },
            created,
        )
        .unwrap();
    }
    let ids: Vec<String> = Session::list(&root)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, vec!["1700000000300", "1700000000200", "1700000000100"]);
}
