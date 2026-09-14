//! What the app is currently looking at.
//!
//! One session is open at a time (ADR 0008). The replay cursor, its bars and
//! its event log live behind a single lock, so a command can never read a
//! cursor belonging to a different session than the candles it returns.
//!
//! Bars and events are held in memory because trading state is recomputed from
//! scratch on every cursor move (see [`replay_engine::sim`]). That is what
//! makes stepping backward exact, and it is only affordable if the inputs are
//! not re-read from disk each time.

use replay_core::{Bar, Candle, Error, Instrument, Result, Timeframe, Timestamp};
use replay_data::Store;
use replay_engine::session::LoggedEvent;
use replay_engine::sim::{simulate, NoTicks, SimConfig, SimState};
use replay_engine::{gaps, Gap, Replay, Session};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::dto::{self, ViewDto, WINDOW};

pub struct Open {
    pub session: Session,
    pub replay: Replay,
    /// Every base bar in the session range, ascending.
    pub bars: Vec<Bar>,
    pub events: Vec<LoggedEvent>,
}

impl Open {
    pub fn config(&self) -> SimConfig {
        let i = &self.session.instrument;
        SimConfig {
            starting_balance: self.session.balance,
            // Points are an instrument-relative unit; the engine works in price.
            spread: self.session.spread_points * i.point,
            commission_per_unit: self.session.commission_per_unit,
            multiplier: i.multiplier,
        }
    }

    /// Trading state as of the cursor, rebuilt from the log every time.
    ///
    /// Ticks are not consulted yet: no session pins any, so an ambiguous bar
    /// takes the pessimistic stop-first assumption and is flagged (spec §5.3).
    pub fn sim(&self) -> SimState {
        simulate(
            self.config(),
            &self.bars,
            &self.events,
            self.replay.cursor(),
            &NoTicks,
        )
    }

    /// Close of the bar the cursor sits on: the last price the trader can see.
    pub fn last_price(&self) -> Option<f64> {
        let cursor = self.replay.cursor();
        self.bars
            .iter()
            .rev()
            .find(|b| b.ts <= cursor)
            .map(|b| b.close)
    }
}

pub struct Inner {
    pub store: Store,
    pub open: Option<Open>,
    pub timeframe: Timeframe,
}

pub struct AppState {
    pub root: PathBuf,
    pub inner: Mutex<Inner>,
}

impl AppState {
    pub fn new(root: PathBuf) -> Result<AppState> {
        Ok(AppState {
            root,
            inner: Mutex::new(Inner {
                store: Store::open()?,
                open: None,
                timeframe: Timeframe::M1,
            }),
        })
    }
}

impl Inner {
    pub fn opened(&mut self) -> Result<&mut Open> {
        self.open
            .as_mut()
            .ok_or_else(|| Error::Data("no session is open".into()))
    }

    pub fn peek(&self) -> Result<&Open> {
        self.open
            .as_ref()
            .ok_or_else(|| Error::Data("no session is open".into()))
    }

    /// Roughly `count` candles of history before `cursor`.
    ///
    /// Approximate on purpose: gaps mean a fixed time span holds a variable
    /// number of candles, and the caller keeps only the last `count` anyway.
    /// Scanning the whole session range on every step would be wasteful for a
    /// year-long session and buy nothing.
    fn window_start(tf: Timeframe, cursor: Timestamp, count: usize) -> Timestamp {
        let span = match tf.fixed_ms() {
            Some(ms) => ms,
            None if tf == Timeframe::W1 => 7 * Timestamp::DAY,
            None => Timestamp::DAY,
        };
        Timestamp(cursor.0.saturating_sub(span * count as i64))
    }

    /// The candle window, its gaps and the trading state, as of the cursor.
    pub fn view(&mut self, root: &Path) -> Result<ViewDto> {
        let tf = self.timeframe;
        let open = self.peek()?;
        let cursor = open.replay.cursor();
        let instrument = open.session.instrument.clone();
        let session_id = open.session.id.clone();
        let bars_path = open.session.bars_path(root);
        let first = open.replay.first();

        let from = Self::window_start(tf, cursor, WINDOW).max(first);
        let mut candles =
            self.store
                .candles(&bars_path, tf, instrument.session_tz, from, cursor)?;
        if candles.len() > WINDOW {
            candles.drain(..candles.len() - WINDOW);
        }

        let open = self.peek()?;
        let gaps = gaps_in(&open.bars, &instrument, &candles, cursor);

        Ok(ViewDto {
            session_id,
            instrument: (&instrument).into(),
            timeframe: tf.label().to_string(),
            cursor: dto::cursor_dto(&open.replay, instrument.session_tz),
            candles,
            gaps,
            trading: dto::trading_dto(open),
        })
    }

    /// Gaps among the bars revealed between `previous` and the cursor.
    ///
    /// Play mode pushes candles incrementally rather than reloading the window,
    /// so without this a data gap crossed mid-playback would go unreported
    /// until something else forced a reload — and a silent hole in the data is
    /// exactly what spec §5.4 exists to prevent.
    pub fn gaps_revealed(&self, previous: Timestamp) -> Result<Vec<Gap>> {
        let open = self.peek()?;
        let cursor = open.replay.cursor();
        let times: Vec<Timestamp> = open
            .bars
            .iter()
            .filter(|b| b.ts >= previous && b.ts <= cursor)
            .map(|b| b.ts)
            .collect();
        Ok(gaps::find(&times, open.session.instrument.market))
    }

    /// Candles from the bucket `previous` sat in, up to the cursor.
    ///
    /// After a forward step this is everything that changed: the candle that
    /// was forming, plus any that opened since.
    pub fn tail_since(&mut self, root: &Path, previous: Timestamp) -> Result<Vec<Candle>> {
        let tf = self.timeframe;
        let open = self.peek()?;
        let bars_path = open.session.bars_path(root);
        let cursor = open.replay.cursor();
        let tz = open.session.instrument.session_tz;
        let from = tf.bucket_start(previous, tz);
        self.store.candles(&bars_path, tf, tz, from, cursor)
    }
}

/// Gaps among the base bars the visible window covers (spec §5.4).
fn gaps_in(
    bars: &[Bar],
    instrument: &Instrument,
    candles: &[Candle],
    cursor: Timestamp,
) -> Vec<Gap> {
    let Some(first) = candles.first() else {
        return Vec::new();
    };
    let times: Vec<Timestamp> = bars
        .iter()
        .filter(|b| b.ts >= first.ts && b.ts <= cursor)
        .map(|b| b.ts)
        .collect();
    gaps::find(&times, instrument.market)
}
