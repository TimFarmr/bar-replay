//! Console rendering. Prices use the instrument's own precision so what is
//! printed matches what the app would export (ADR 0010).

use replay_core::{Candle, Instrument, Timeframe, Timestamp};

pub fn print_candles(
    inst: &Instrument,
    tf: Timeframe,
    cursor: Timestamp,
    candles: &[Candle],
    limit: usize,
) {
    println!(
        "{}/{} {} — cursor {} UTC, session timezone {}",
        inst.provider,
        inst.symbol,
        tf.label(),
        cursor,
        inst.session_tz
    );
    if candles.is_empty() {
        println!("  no candles at or before the cursor");
        return;
    }
    let d = inst.price_decimals as usize;
    println!(
        "{:<20} {:>12} {:>12} {:>12} {:>12} {:>12}  STATE",
        "OPEN TIME (UTC)", "OPEN", "HIGH", "LOW", "CLOSE", "VOLUME"
    );
    let skip = candles.len().saturating_sub(limit);
    for c in &candles[skip..] {
        println!(
            "{:<20} {:>12.d$} {:>12.d$} {:>12.d$} {:>12.d$} {:>12.2}  {}",
            c.ts.to_string(),
            c.open,
            c.high,
            c.low,
            c.close,
            c.volume,
            if c.complete { "closed" } else { "forming" },
            d = d
        );
    }
    if skip > 0 {
        println!("({skip} earlier candles not shown; --limit to change)");
    }
}
