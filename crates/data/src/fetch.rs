//! Downloading a date range into the local Parquet cache.
//!
//! Shared by the CLI and the desktop app so there is exactly one description of
//! how history is fetched. Progress is reported through a callback rather than
//! printed, because only the caller knows whether it is talking to a terminal
//! or a window.
//!
//! The range is split into single days and fetched concurrently: Dukascopy
//! serves one file per day and can take many seconds each, so a month fetched
//! strictly one at a time would blow through the five-minute rule the whole
//! project is optimised for. Concurrency is whatever the adapter says it can
//! take — the user is the subscriber and their rate limit is not ours to spend.

use crate::Store;
use replay_core::{Bar, Instrument, Provider, Result, Timestamp};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Days of bars held in memory before a flush. Every upsert rewrites the whole
/// Parquet file, so flushing per day would be quadratic over a long range; 30
/// days of 1-minute bars is a few MB and bounds a crash to losing a month of
/// downloading.
const FLUSH_DAYS: usize = 30;

#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub days_done: usize,
    pub days_total: usize,
    pub bars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    /// Bars actually downloaded in this run.
    pub fetched: usize,
    /// Rows in the cache afterwards, including anything already there.
    pub cached: i64,
    /// Days the provider had nothing at all for.
    pub empty_days: usize,
}

pub fn range(
    provider: &dyn Provider,
    inst: &Instrument,
    from: Timestamp,
    to: Timestamp,
    store: &Store,
    path: &Path,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<Summary> {
    let days = split_days(from, to);
    let days_total = days.len();
    let mut fetched = 0usize;
    let mut empty_days = 0usize;
    let mut days_done = 0usize;

    for batch in days.chunks(FLUSH_DAYS) {
        let mut per_day = fetch_batch(provider, inst, batch)?;
        empty_days += per_day.iter().filter(|d| d.is_empty()).count();
        let mut flat: Vec<Bar> = per_day.drain(..).flatten().collect();
        flat.sort_by_key(|b| b.ts);
        fetched += flat.len();
        if !flat.is_empty() {
            store.upsert_bars(path, &flat)?;
        }
        days_done += batch.len();
        on_progress(Progress {
            days_done,
            days_total,
            bars: fetched,
        });
    }

    Ok(Summary {
        fetched,
        cached: store.coverage(path)?.map_or(0, |c| c.bars),
        empty_days,
    })
}

/// `[from, to)` cut on UTC midnights, with both ends possibly partial.
fn split_days(from: Timestamp, to: Timestamp) -> Vec<(Timestamp, Timestamp)> {
    let mut out = Vec::new();
    let mut day = from;
    while day < to {
        let next = Timestamp((day.floor(Timestamp::DAY).0 + Timestamp::DAY).min(to.0));
        out.push((day, next));
        day = next;
    }
    out
}

/// Fetch every day in `batch` concurrently, preserving order. The first error
/// fails the batch: a partial range silently presented as complete is exactly
/// the kind of dishonesty this app exists to avoid.
fn fetch_batch(
    provider: &dyn Provider,
    inst: &Instrument,
    batch: &[(Timestamp, Timestamp)],
) -> Result<Vec<Vec<Bar>>> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Result<Vec<Bar>>>>> =
        batch.iter().map(|_| Mutex::new(None)).collect();

    std::thread::scope(|scope| {
        for _ in 0..provider.max_concurrency().clamp(1, batch.len().max(1)) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= batch.len() {
                    break;
                }
                let (start, end) = batch[i];
                *slots[i].lock().unwrap() = Some(provider.minutes(inst, start, end));
            });
        }
    });

    slots
        .into_iter()
        .map(|slot| slot.into_inner().unwrap().expect("every day was claimed"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_cut_on_midnights_with_partial_ends() {
        let from = Timestamp(Timestamp::DAY + 5 * Timestamp::HOUR);
        let to = Timestamp(3 * Timestamp::DAY + Timestamp::HOUR);
        let days = split_days(from, to);
        assert_eq!(days.len(), 3);
        assert_eq!(days[0], (from, Timestamp(2 * Timestamp::DAY)));
        assert_eq!(
            days[1],
            (Timestamp(2 * Timestamp::DAY), Timestamp(3 * Timestamp::DAY))
        );
        assert_eq!(days[2], (Timestamp(3 * Timestamp::DAY), to));
        // Contiguous and covering, with no overlap.
        for pair in days.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
    }

    #[test]
    fn a_range_inside_one_day_is_a_single_request() {
        let from = Timestamp(Timestamp::DAY);
        let to = Timestamp(Timestamp::DAY + Timestamp::HOUR);
        assert_eq!(split_days(from, to), vec![(from, to)]);
    }

    #[test]
    fn an_empty_range_needs_no_requests() {
        assert!(split_days(Timestamp(Timestamp::DAY), Timestamp(Timestamp::DAY)).is_empty());
    }
}
