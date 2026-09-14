//! The IPC surface.
//!
//! Every command that moves the cursor also writes the move to the session's
//! event log (ADR 0010/0011), so closing the app mid-replay loses nothing and
//! the log stays a faithful record of what the user actually did.

use replay_core::{Timeframe, Timestamp};
use replay_data::{catalog, fetch, paths, providers};
use replay_engine::session::{Account, Event, Session};
use replay_engine::{OrderKind, Replay, Side};
use tauri::State;

use crate::dto::*;
use crate::state::{AppState, Open};

/// Tauri needs a serializable error; the UI shows this string verbatim.
type Answer<T> = std::result::Result<T, String>;

fn oops(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn instruments(state: State<'_, AppState>) -> Vec<InstrumentDto> {
    catalog::available(&state.root)
        .iter()
        .map(InstrumentDto::from)
        .collect()
}

#[tauri::command]
pub fn coverage(
    state: State<'_, AppState>,
    provider: String,
    symbol: String,
) -> Answer<Option<CoverageDto>> {
    let inner = state.inner.lock().unwrap();
    let path = paths::base_parquet(&state.root, &provider, &symbol);
    Ok(inner.store.coverage(&path).map_err(oops)?.map(coverage_dto))
}

/// Download history straight from the provider (ADR 0007).
///
/// Runs on Tauri's command thread pool, so a slow provider blocks this call but
/// never the window.
#[tauri::command]
pub fn fetch_range(
    state: State<'_, AppState>,
    provider: String,
    symbol: String,
    from: i64,
    to: i64,
) -> Answer<FetchSummaryDto> {
    let instrument = catalog::resolve(&state.root, &provider, &symbol)
        .ok_or_else(|| format!("{provider}/{symbol} is not a known instrument"))?;
    let adapter = providers::by_id(&provider).map_err(oops)?;
    let path = paths::base_parquet(&state.root, &provider, &symbol);
    let inner = state.inner.lock().unwrap();

    let summary = fetch::range(
        adapter.as_ref(),
        &instrument,
        Timestamp(from),
        Timestamp(to),
        &inner.store,
        &path,
        &mut |_| {},
    )
    .map_err(oops)?;

    Ok(FetchSummaryDto {
        fetched: summary.fetched,
        cached: summary.cached,
        empty_days: summary.empty_days,
    })
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> Answer<Vec<SessionSummaryDto>> {
    Ok(Session::list(&state.root)
        .map_err(oops)?
        .iter()
        .map(session_summary)
        .collect())
}

/// What the setup screen collects before a session can start.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSession {
    pub provider: String,
    pub symbol: String,
    pub from: i64,
    pub to: i64,
    pub balance: f64,
    /// Spread in instrument points. For a trade-only provider this is the
    /// trader's own assumption, and the session UI labels it as such (§5.3).
    pub spread_points: f64,
    pub commission_per_unit: f64,
}

#[tauri::command]
pub fn create_session(state: State<'_, AppState>, request: NewSession) -> Answer<ViewDto> {
    let instrument =
        catalog::resolve(&state.root, &request.provider, &request.symbol).ok_or_else(|| {
            format!(
                "{}/{} is not a known instrument",
                request.provider, request.symbol
            )
        })?;
    let now = chrono::Utc::now().timestamp_millis();
    let session = Session::create(
        &state.root,
        &instrument,
        Timestamp(request.from),
        Timestamp(request.to),
        Account {
            balance: request.balance,
            spread_points: request.spread_points,
            commission_per_unit: request.commission_per_unit,
        },
        now,
    )
    .map_err(oops)?;
    open_by_id(&state, session.id)
}

#[tauri::command]
pub fn open_session(state: State<'_, AppState>, id: String) -> Answer<ViewDto> {
    open_by_id(&state, id)
}

fn open_by_id(state: &State<'_, AppState>, id: String) -> Answer<ViewDto> {
    let session = Session::load(&state.root, &id).map_err(oops)?;
    let mut inner = state.inner.lock().unwrap();

    let bars = inner
        .store
        .bars(
            &session.bars_path(&state.root),
            session.range_from,
            session.range_to,
        )
        .map_err(oops)?;
    let mut replay = Replay::new(bars.iter().map(|b| b.ts).collect())
        .ok_or_else(|| "this session has no bars to replay".to_string())?;

    // Resume exactly where the user left off (spec §5.6).
    if let Some(cursor) = session.resume_cursor(&state.root).map_err(oops)? {
        replay.jump_to(cursor);
    }
    let events = session.events(&state.root).map_err(oops)?;

    inner.open = Some(Open {
        session,
        replay,
        bars,
        events,
    });
    inner.view(&state.root).map_err(oops)
}

#[tauri::command]
pub fn set_timeframe(state: State<'_, AppState>, timeframe: String) -> Answer<ViewDto> {
    let tf =
        Timeframe::parse(&timeframe).ok_or_else(|| format!("unknown timeframe {timeframe}"))?;
    let mut inner = state.inner.lock().unwrap();
    inner.timeframe = tf;
    inner.view(&state.root).map_err(oops)
}

/// The current window, used after any move that can remove candles.
#[tauri::command]
pub fn view(state: State<'_, AppState>) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    inner.view(&state.root).map_err(oops)
}

/// Reveal the next `count` base bars.
///
/// Returns only the candles that changed so the chart can absorb them without
/// a reload, which is what keeps play mode smooth.
#[tauri::command]
pub fn step_forward(state: State<'_, AppState>, count: usize) -> Answer<StepDto> {
    let mut inner = state.inner.lock().unwrap();
    let previous = {
        let open = inner.opened().map_err(oops)?;
        let previous = open.replay.cursor();
        open.replay.advance(count.max(1));
        previous
    };
    log_cursor(&state, &mut inner)?;

    let tail = inner.tail_since(&state.root, previous).map_err(oops)?;
    let new_gaps = inner.gaps_revealed(previous).map_err(oops)?;
    let open = inner.peek().map_err(oops)?;
    let cursor = crate::dto::cursor_dto(&open.replay, open.session.instrument.session_tz);
    Ok(StepDto {
        cursor,
        tail,
        trading: crate::dto::trading_dto(open),
        new_gaps,
    })
}

/// Hide the newest `count` base bars.
///
/// Candles disappear, which no incremental chart update can express, so the
/// caller gets a whole fresh window.
#[tauri::command]
pub fn step_back(state: State<'_, AppState>, count: usize) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    inner.opened().map_err(oops)?.replay.rewind(count.max(1));
    log_cursor(&state, &mut inner)?;
    inner.view(&state.root).map_err(oops)
}

#[tauri::command]
pub fn jump_to(state: State<'_, AppState>, timestamp: i64) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    inner
        .opened()
        .map_err(oops)?
        .replay
        .jump_to(Timestamp(timestamp));
    log_cursor(&state, &mut inner)?;
    inner.view(&state.root).map_err(oops)
}

/// Record where the cursor landed. Keyed to the cursor, never to wall-clock
/// time, so replaying the log is deterministic (ADR 0010).
fn log_cursor(state: &State<'_, AppState>, inner: &mut crate::state::Inner) -> Answer<()> {
    let to = inner.opened().map_err(oops)?.replay.cursor();
    record(state, inner, Event::CursorSet { to })
}

/// Append an event to the log on disk and to the in-memory copy the simulator
/// reads. Both or neither: a divergence would make the replayed session differ
/// from the one on screen.
pub fn record(
    state: &State<'_, AppState>,
    inner: &mut crate::state::Inner,
    event: Event,
) -> Answer<()> {
    let open = inner.opened().map_err(oops)?;
    let cursor = open.replay.cursor();
    let Open {
        session,
        events,
        replay: _,
        bars: _,
    } = open;
    session
        .record(&state.root, events, cursor, event)
        .map_err(oops)
}

/// Place an order at the current cursor.
///
/// A market order fills against the bar the trader is looking at; limit and
/// stop orders rest and are tested from the next bar onward, so nothing can
/// fill on price action already on screen.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn place_order(
    state: State<'_, AppState>,
    side: String,
    kind: String,
    qty: f64,
    price: Option<f64>,
    sl: Option<f64>,
    tp: Option<f64>,
) -> Answer<ViewDto> {
    let side = match side.as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        other => return Err(format!("unknown side {other:?}")),
    };
    let kind = match kind.as_str() {
        "market" => OrderKind::Market,
        "limit" => OrderKind::Limit,
        "stop" => OrderKind::Stop,
        other => return Err(format!("unknown order kind {other:?}")),
    };
    // NaN is not caught by `qty <= 0.0` alone, and a NaN quantity would
    // poison every number downstream of the fill.
    if !qty.is_finite() || qty <= 0.0 {
        return Err("quantity must be a number greater than zero".into());
    }
    if kind != OrderKind::Market && price.is_none() {
        return Err("a limit or stop order needs a price".into());
    }

    let mut inner = state.inner.lock().unwrap();
    record(
        &state,
        &mut inner,
        Event::OrderPlace {
            side,
            kind,
            qty,
            price,
            sl,
            tp,
        },
    )?;
    inner.view(&state.root).map_err(oops)
}

/// Set stop-loss and take-profit outright. `order` of `None` targets the open
/// position, which is how a move to break-even is expressed.
#[tauri::command]
pub fn modify(
    state: State<'_, AppState>,
    order: Option<u64>,
    sl: Option<f64>,
    tp: Option<f64>,
) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    record(&state, &mut inner, Event::OrderModify { order, sl, tp })?;
    inner.view(&state.root).map_err(oops)
}

#[tauri::command]
pub fn cancel_order(state: State<'_, AppState>, order: u64) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    record(&state, &mut inner, Event::OrderCancel { order })?;
    inner.view(&state.root).map_err(oops)
}

/// Close the position at market. `qty` of `None` closes all of it.
#[tauri::command]
pub fn close_position(state: State<'_, AppState>, qty: Option<f64>) -> Answer<ViewDto> {
    let mut inner = state.inner.lock().unwrap();
    record(&state, &mut inner, Event::PositionClose { qty })?;
    inner.view(&state.root).map_err(oops)
}
