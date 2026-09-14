//! Settings: bring-your-own-key, CSV import and cache management (M5).
//!
//! API keys go to the OS keychain and nowhere else — not to a config file, not
//! to a log, and not to any server of ours (ADR 0007). Nothing here ever
//! returns a key back to the UI; the UI only ever learns whether one is set.

use replay_core::{Instrument, Market, SpreadMode};
use replay_data::providers::csv;
use replay_data::{catalog, keychain, paths};
use tauri::State;

use crate::dto::{coverage_dto, CoverageDto, InstrumentDto, ProviderDto};
use crate::state::AppState;

type Answer<T> = std::result::Result<T, String>;

fn oops(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Whether a key is stored, never the key itself.
#[tauri::command]
pub fn has_api_key(provider: String) -> bool {
    keychain::get(&provider).is_some()
}

/// Every adapter and whether it currently has a key.
///
/// The UI reads this instead of naming providers itself, so adding one does
/// not mean editing the settings screen.
#[tauri::command]
pub fn providers() -> Vec<ProviderDto> {
    replay_data::providers::registry()
        .iter()
        .map(|p| ProviderDto {
            id: p.id.to_string(),
            label: p.label.to_string(),
            needs_key: p.needs_key,
            free: p.free,
            from_file: p.from_file,
            has_key: p.needs_key && keychain::get(p.id).is_some(),
        })
        .collect()
}

#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Answer<()> {
    keychain::set(&provider, key.trim()).map_err(oops)
}

#[tauri::command]
pub fn clear_api_key(provider: String) -> Answer<()> {
    keychain::delete(&provider).map_err(oops)
}

/// Everything the user can start a session on, built-in or imported.
#[tauri::command]
pub fn available_instruments(state: State<'_, AppState>) -> Vec<InstrumentDto> {
    catalog::available(&state.root)
        .iter()
        .map(InstrumentDto::from)
        .collect()
}

/// Open a native file picker and return the chosen path, or `None` if the user
/// cancelled. Typing a filesystem path is not something this app's users
/// should ever have to do.
#[tauri::command]
pub fn pick_csv() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("CSV", &["csv", "txt"])
        .set_title("Choose a CSV of candles")
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

/// Import a CSV of candles as a new instrument.
///
/// Rows must be strictly ascending with no duplicates. A file that breaks that
/// is rejected with the offending line number rather than quietly sorted — a
/// silent sort would fabricate a price history that never happened (spec §4).
#[tauri::command]
pub fn import_csv(
    state: State<'_, AppState>,
    path: String,
    symbol: String,
    price_decimals: u32,
) -> Answer<CoverageDto> {
    let symbol = symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err("give the instrument a name, for example MYDATA".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    let bars = csv::parse(&text).map_err(oops)?;
    if bars.is_empty() {
        return Err("that file has no candles in it".into());
    }

    let decimals = price_decimals.min(10);
    let instrument = Instrument {
        provider: csv::ID.to_string(),
        symbol: symbol.clone(),
        price_decimals: decimals,
        point: 10f64.powi(-(decimals as i32)),
        multiplier: 1.0,
        quote_currency: String::new(),
        session_tz: chrono_tz::Tz::UTC,
        // The file carries traded prices only, so any spread is the user's
        // own assumption and the session UI says so (spec §5.3).
        spread_mode: SpreadMode::Synthetic,
        market: Market::Continuous,
    };

    catalog::save(&state.root, &instrument).map_err(oops)?;
    let target = paths::base_parquet(&state.root, csv::ID, &symbol);
    let inner = state.inner.lock().unwrap();
    inner.store.upsert_bars(&target, &bars).map_err(oops)?;

    inner
        .store
        .coverage(&target)
        .map_err(oops)?
        .map(coverage_dto)
        .ok_or_else(|| "the import produced no candles".to_string())
}

/// Forget the downloaded candles for one instrument.
///
/// Deliberately manual: nothing is evicted automatically (spec §4), so the
/// disk-space trade-off stays the user's to make. Sessions keep their own
/// pinned copy and are unaffected (ADR 0012).
#[tauri::command]
pub fn clear_cache(state: State<'_, AppState>, provider: String, symbol: String) -> Answer<()> {
    let dir = paths::instrument_dir(&state.root, &provider, &symbol);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).map_err(oops)
}
