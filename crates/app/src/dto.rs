//! What crosses the IPC boundary.
//!
//! These types exist so the webview receives exactly what it needs to draw and
//! nothing more. In particular the candle list is always the cursor-filtered
//! window: the UI is never handed future data and asked to hide it (spec §5.1).

use replay_core::{Candle, Instrument, Market, SpreadMode, Timestamp};
use replay_engine::order::{Assumption, Reason, Role, Side};
use replay_engine::Gap;
use serde::Serialize;

/// How many candles the chart window holds. Enough to fill a wide screen at any
/// timeframe without shipping a year of history on every step.
pub const WINDOW: usize = 400;

fn label(ts: Timestamp) -> String {
    ts.to_string()
}

/// Spec §4: session boundaries are reasoned about in the instrument's own
/// timezone, and that timezone is always shown, never silently assumed. The
/// cursor is therefore rendered in it rather than in UTC, and the UI prints the
/// zone name beside it.
fn label_in(ts: Timestamp, tz: chrono_tz::Tz) -> String {
    chrono::DateTime::from_timestamp_millis(ts.0)
        .map(|dt| {
            dt.with_timezone(&tz)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| label(ts))
}

/// One adapter, as the settings and setup screens see it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDto {
    pub id: String,
    pub label: String,
    pub needs_key: bool,
    pub free: bool,
    pub from_file: bool,
    /// Whether a key is stored for it. Never the key itself.
    pub has_key: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentDto {
    pub provider: String,
    pub symbol: String,
    pub price_decimals: u32,
    pub quote_currency: String,
    pub session_tz: String,
    /// Shown verbatim in the session UI so a synthetic spread is never mistaken
    /// for historical fact (spec §5.3).
    pub spread_mode: SpreadMode,
    /// Lets the setup screen prefer an always-open instrument for a first run
    /// instead of naming one in the UI.
    pub market: Market,
}

impl From<&Instrument> for InstrumentDto {
    fn from(i: &Instrument) -> Self {
        InstrumentDto {
            provider: i.provider.clone(),
            symbol: i.symbol.clone(),
            price_decimals: i.price_decimals,
            quote_currency: i.quote_currency.clone(),
            session_tz: i.session_tz.to_string(),
            spread_mode: i.spread_mode,
            market: i.market,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageDto {
    pub from: i64,
    pub to: i64,
    pub from_label: String,
    pub to_label: String,
    pub bars: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchSummaryDto {
    pub fetched: usize,
    pub cached: i64,
    pub empty_days: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub id: String,
    pub provider: String,
    pub symbol: String,
    pub created_ms: i64,
    pub range_from: i64,
    pub range_to: i64,
    pub range_label: String,
    pub balance: f64,
}

/// Where the replay currently stands. `position` and `bars` are for a progress
/// bar only — navigation is always by timestamp (spec §5.1).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorDto {
    pub cursor: i64,
    /// Formatted in the instrument's session timezone, which `timezone` names.
    pub cursor_label: String,
    pub timezone: String,
    pub position: usize,
    pub bars: usize,
    pub at_start: bool,
    pub at_end: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDto {
    pub session_id: String,
    pub instrument: InstrumentDto,
    pub timeframe: String,
    pub cursor: CursorDto,
    pub candles: Vec<Candle>,
    /// Gaps inside the visible window, already classified (spec §5.4).
    pub gaps: Vec<Gap>,
    pub trading: TradingDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionDto {
    pub side: Side,
    pub qty: f64,
    pub avg_entry: f64,
    pub sl: Option<f64>,
    pub tp: Option<f64>,
    pub opened_label: String,
    /// Profit if closed at the last visible price.
    pub unrealised: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderDto {
    pub id: u64,
    pub side: Side,
    pub kind: String,
    pub qty: f64,
    pub price: Option<f64>,
    pub sl: Option<f64>,
    pub tp: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeDto {
    pub seq: usize,
    pub when: String,
    pub side: Side,
    pub qty: f64,
    pub price: f64,
    pub role: Role,
    pub reason: Reason,
    /// `sl_first` marks a trade whose outcome rested on the pessimistic
    /// assumption rather than on data. The UI must show it (spec §5.3).
    pub assumption: Assumption,
    pub commission: f64,
    pub realised: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradingDto {
    pub balance: f64,
    /// Balance plus any open profit.
    pub equity: f64,
    pub last_price: Option<f64>,
    pub position: Option<PositionDto>,
    pub working: Vec<OrderDto>,
    pub trades: Vec<TradeDto>,
}

pub fn trading_dto(open: &crate::state::Open) -> TradingDto {
    let st = open.sim();
    let tz = open.session.instrument.session_tz;
    let multiplier = open.session.instrument.multiplier;
    let last = open.last_price();

    let position = st.position.map(|p| PositionDto {
        side: p.side,
        qty: p.qty,
        avg_entry: p.avg_entry,
        sl: p.sl,
        tp: p.tp,
        opened_label: label_in(p.opened, tz),
        unrealised: last.map_or(0.0, |px| p.unrealised(px, multiplier)),
    });
    let equity = st.balance + position.as_ref().map_or(0.0, |p| p.unrealised);

    TradingDto {
        balance: st.balance,
        equity,
        last_price: last,
        position,
        working: st
            .working
            .iter()
            .map(|o| OrderDto {
                id: o.id,
                side: o.side,
                kind: format!("{:?}", o.kind).to_lowercase(),
                qty: o.qty,
                price: o.price,
                sl: o.sl,
                tp: o.tp,
            })
            .collect(),
        trades: st
            .trades
            .iter()
            .map(|t| TradeDto {
                seq: t.seq,
                when: label_in(t.cursor, tz),
                side: t.side,
                qty: t.qty,
                price: t.price,
                role: t.role,
                reason: t.reason,
                assumption: t.assumption,
                commission: t.commission,
                realised: t.realised,
            })
            .collect(),
    }
}

/// The answer to a forward step: only the candles that changed.
///
/// Stepping forward either extends the newest candle or starts one, so the UI
/// can push these straight into the chart without reloading and losing the
/// user's zoom.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepDto {
    pub cursor: CursorDto,
    pub tail: Vec<Candle>,
    /// Stepping can fire a stop or a target, so trading state always rides
    /// along: the UI must never show a stale position.
    pub trading: TradingDto,
    /// Gaps uncovered by this step, so a data gap crossed during playback is
    /// reported the moment it is revealed (spec §5.4).
    pub new_gaps: Vec<Gap>,
}

pub fn coverage_dto(c: replay_data::store::Coverage) -> CoverageDto {
    CoverageDto {
        from: c.from.0,
        to: c.to.0,
        from_label: label(c.from),
        to_label: label(c.to),
        bars: c.bars,
    }
}

pub fn cursor_dto(replay: &replay_engine::Replay, tz: chrono_tz::Tz) -> CursorDto {
    CursorDto {
        cursor: replay.cursor().0,
        cursor_label: label_in(replay.cursor(), tz),
        timezone: tz.to_string(),
        position: replay.position(),
        bars: replay.bars(),
        at_start: replay.at_start(),
        at_end: replay.at_end(),
    }
}

pub fn session_summary(s: &replay_engine::Session) -> SessionSummaryDto {
    SessionSummaryDto {
        id: s.id.clone(),
        provider: s.instrument.provider.clone(),
        symbol: s.instrument.symbol.clone(),
        created_ms: s.created_ms,
        range_from: s.range_from.0,
        range_to: s.range_to.0,
        range_label: format!("{} .. {}", label(s.range_from), label(s.range_to)),
        balance: s.balance,
    }
}
