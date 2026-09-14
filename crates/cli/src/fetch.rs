//! `bar-replay fetch` — the terminal's view of [`replay_data::fetch`].

use replay_core::{Instrument, Result, Timestamp};
use replay_data::{fetch, paths, providers, Store};

pub fn run(inst: &Instrument, from: Timestamp, to: Timestamp) -> Result<()> {
    let provider = providers::by_id(&inst.provider)?;
    let root = paths::root();
    let path = paths::base_parquet(&root, &inst.provider, &inst.symbol);
    let store = Store::open()?;

    println!(
        "fetching {}/{} {} .. {} UTC",
        inst.provider, inst.symbol, from, to
    );
    println!("  direct from the provider; no server of ours is involved (ADR 0007)");

    let summary = fetch::range(provider.as_ref(), inst, from, to, &store, &path, &mut |p| {
        // One line per flush, not per day: a year would otherwise scroll
        // the terminal for no benefit.
        if p.days_total > 30 {
            println!(
                "  {}/{} days, {} bars so far",
                p.days_done, p.days_total, p.bars
            );
        }
    })?;

    println!(
        "done: {} bars fetched, cache now holds {}",
        summary.fetched, summary.cached
    );
    if summary.empty_days > 0 {
        println!(
            "  {} day(s) had no data at all (weekends and holidays are expected; \
             they stay gaps, never filled)",
            summary.empty_days
        );
    }
    println!("  {}", path.display());
    Ok(())
}
