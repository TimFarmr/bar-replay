//! Review commands: statistics, the journal, and local export.
//!
//! All of it is derived from the ledger and the event log as of the cursor, so
//! reviewing a session halfway through shows what the trader knew halfway
//! through — not the finished picture.

use replay_engine::journal;
use replay_engine::order::RoundTrip;
use replay_engine::session::Event;
use replay_engine::stats::{self, Equity, Summary};
use replay_engine::JournalEntry;
use tauri::State;

use crate::dto::ViewDto;
use crate::state::AppState;

type Answer<T> = std::result::Result<T, String>;

fn oops(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDto {
    pub summary: Summary,
    pub round_trips: Vec<RoundTrip>,
    pub equity: Vec<Equity>,
    pub journal: Vec<JournalEntry>,
    pub tags: Vec<String>,
    pub price_decimals: u32,
}

#[tauri::command]
pub fn review(state: State<'_, AppState>) -> Answer<ReviewDto> {
    let inner = state.inner.lock().unwrap();
    let open = inner.peek().map_err(oops)?;
    let sim = open.sim();
    let multiplier = open.session.instrument.multiplier;

    let round_trips = stats::round_trips(&sim.trades, multiplier);
    let journal = journal::entries(&open.events, open.replay.cursor());

    Ok(ReviewDto {
        summary: stats::summary(open.session.balance, &round_trips),
        equity: stats::equity_curve(open.session.balance, &sim.trades),
        tags: journal::tags(&journal),
        round_trips,
        journal,
        price_decimals: open.session.instrument.price_decimals,
    })
}

/// Write a note at the current cursor. Passing an existing `note` id edits that
/// note; omitting it starts a new one.
#[tauri::command]
pub fn add_note(
    state: State<'_, AppState>,
    note: Option<u64>,
    text: String,
    tags: Vec<String>,
    trade: Option<usize>,
) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    let id = match note {
        Some(id) => id,
        // A fresh id is one past the highest note already written, so an edit
        // can always be told from a new note.
        None => {
            let open = inner.peek().map_err(oops)?;
            journal::entries(&open.events, replay_core::Timestamp(i64::MAX))
                .iter()
                .map(|e| e.id)
                .max()
                .unwrap_or(0)
                + 1
        }
    };
    crate::commands::record(
        &state,
        &mut inner,
        Event::NoteSet {
            note: id,
            text,
            tags,
            trade,
        },
    )?;
    inner.view(&state.root).map_err(oops)
}

/// Write the session's results next to the session itself and return the
/// folder. Local files only: nothing leaves the machine.
#[tauri::command]
pub fn export_session(state: State<'_, AppState>) -> Answer<String> {
    use replay_engine::export;

    let inner = state.inner.lock().unwrap();
    let open = inner.peek().map_err(oops)?;
    let sim = open.sim();
    let instrument = &open.session.instrument;
    let decimals = instrument.price_decimals as usize;

    let trips = stats::round_trips(&sim.trades, instrument.multiplier);
    let notes = journal::entries(&open.events, open.replay.cursor());
    let summary = stats::summary(open.session.balance, &trips);

    let dir = open.session.dir(&state.root).join("export");
    std::fs::create_dir_all(&dir).map_err(oops)?;
    std::fs::write(
        dir.join("trades.csv"),
        export::trades_csv(&sim.trades, decimals),
    )
    .map_err(oops)?;
    std::fs::write(
        dir.join("round-trips.csv"),
        export::round_trips_csv(&trips, decimals),
    )
    .map_err(oops)?;
    std::fs::write(dir.join("journal.csv"), export::journal_csv(&notes)).map_err(oops)?;
    std::fs::write(
        dir.join("session.json"),
        export::session_json(&summary, &sim.trades, &trips, &notes)?,
    )
    .map_err(oops)?;

    Ok(dir.to_string_lossy().to_string())
}
