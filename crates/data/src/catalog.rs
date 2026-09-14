//! The instruments offered on first launch.
//!
//! Both free providers must work with zero configuration (spec §4), so the
//! symbols, their tick sizes and their session timezones ship in the binary
//! rather than behind a lookup call that could fail offline.

use chrono_tz::Tz;
use replay_core::{Instrument, Market, SpreadMode};

/// `decimals` fixes both display precision and the integer price scale used by
/// the Dukascopy wire format, so the two can never drift apart.
fn fx(symbol: &str, decimals: u32) -> Instrument {
    Instrument {
        provider: "dukascopy".into(),
        symbol: symbol.into(),
        price_decimals: decimals,
        point: 10f64.powi(-(decimals as i32)),
        multiplier: 1.0,
        quote_currency: symbol[3..].into(),
        // FX has no single exchange; the market's own week runs on UTC.
        session_tz: Tz::UTC,
        // Dukascopy publishes real bid and ask (§5.3).
        spread_mode: SpreadMode::Historical,
        market: Market::FxWeek,
    }
}

fn crypto(symbol: &str, quote: &str, decimals: u32) -> Instrument {
    Instrument {
        provider: "binance".into(),
        symbol: symbol.into(),
        price_decimals: decimals,
        point: 10f64.powi(-(decimals as i32)),
        multiplier: 1.0,
        quote_currency: quote.into(),
        session_tz: Tz::UTC,
        // Binance klines are trade prints only: the spread is a user constant
        // and the UI must say so (§5.3).
        spread_mode: SpreadMode::Synthetic,
        market: Market::Continuous,
    }
}

pub fn builtin() -> Vec<Instrument> {
    vec![
        fx("EURUSD", 5),
        fx("GBPUSD", 5),
        fx("AUDUSD", 5),
        fx("NZDUSD", 5),
        fx("USDCHF", 5),
        fx("USDCAD", 5),
        fx("USDJPY", 3),
        fx("EURJPY", 3),
        fx("GBPJPY", 3),
        fx("XAUUSD", 3),
        crypto("BTCUSDT", "USDT", 2),
        crypto("ETHUSDT", "USDT", 2),
        crypto("SOLUSDT", "USDT", 2),
        crypto("BNBUSDT", "USDT", 2),
        crypto("XRPUSDT", "USDT", 4),
    ]
}

pub fn find(provider: &str, symbol: &str) -> Option<Instrument> {
    builtin()
        .into_iter()
        .find(|i| i.provider == provider && i.symbol.eq_ignore_ascii_case(symbol))
}

/// Instruments the user brought themselves (a CSV import) live on disk beside
/// their data, as `instrument.json` (docs/schema.md). Built-in instruments do
/// not need a file; these do, because nothing in the binary describes them.
pub fn save(root: &std::path::Path, inst: &Instrument) -> replay_core::Result<()> {
    let dir = crate::paths::instrument_dir(root, &inst.provider, &inst.symbol);
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(inst)
        .map_err(|e| replay_core::Error::Data(format!("encoding instrument: {e}")))?;
    std::fs::write(dir.join("instrument.json"), json)?;
    Ok(())
}

fn load(root: &std::path::Path, provider: &str, symbol: &str) -> Option<Instrument> {
    let path = crate::paths::instrument_dir(root, provider, symbol).join("instrument.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Everything the user can open: the built-ins plus anything imported.
pub fn available(root: &std::path::Path) -> Vec<Instrument> {
    let mut out = builtin();
    let dir = root.join("instruments");
    let Ok(providers) = std::fs::read_dir(&dir) else {
        return out;
    };
    for provider in providers.flatten() {
        let Ok(symbols) = std::fs::read_dir(provider.path()) else {
            continue;
        };
        for symbol in symbols.flatten() {
            let (Some(p), Some(s)) = (
                provider.file_name().to_str().map(String::from),
                symbol.file_name().to_str().map(String::from),
            ) else {
                continue;
            };
            // A built-in of the same name wins: the shipped definition is the
            // one its adapter was written against.
            if out.iter().any(|i| i.provider == p && i.symbol == s) {
                continue;
            }
            if let Some(inst) = load(root, &p, &s) {
                out.push(inst);
            }
        }
    }
    out
}

/// Resolve against built-ins first, then imported instruments.
pub fn resolve(root: &std::path::Path, provider: &str, symbol: &str) -> Option<Instrument> {
    find(provider, symbol).or_else(|| load(root, provider, symbol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_matches_decimals_for_every_builtin_instrument() {
        for i in builtin() {
            let scaled = i.point * 10f64.powi(i.price_decimals as i32);
            assert!(
                (scaled - 1.0).abs() < 1e-9,
                "{} point {} disagrees with {} decimals",
                i.symbol,
                i.point,
                i.price_decimals
            );
        }
    }

    #[test]
    fn jpy_pairs_use_three_decimals_and_majors_five() {
        assert_eq!(find("dukascopy", "USDJPY").unwrap().price_decimals, 3);
        assert_eq!(find("dukascopy", "EURUSD").unwrap().price_decimals, 5);
        assert_eq!(find("dukascopy", "eurusd").unwrap().symbol, "EURUSD");
        assert!(find("binance", "EURUSD").is_none());
    }

    #[test]
    fn every_dukascopy_instrument_has_a_real_spread_and_binance_does_not() {
        for i in builtin() {
            let expected = if i.provider == "dukascopy" {
                SpreadMode::Historical
            } else {
                SpreadMode::Synthetic
            };
            assert_eq!(i.spread_mode, expected, "{}", i.symbol);
        }
    }
}
