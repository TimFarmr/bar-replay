//! Sessions: a header plus an append-only event log (ADR 0011).
//!
//! Every user action is written to `events.jsonl` the moment it happens and
//! keyed to the **cursor**, never to wall-clock time (ADR 0010). Resuming a
//! session replays that log, which is also what makes the determinism test in
//! spec §6 meaningful: same log, same engine, same ledger.
//!
//! At creation the session copies the base bars it was started with into its
//! own directory (ADR 0012), so a provider silently revising history later can
//! never change what a recorded trade was taken against.

use crate::order::{OrderKind, Side};
use replay_core::{Error, Instrument, Result, Timestamp};
use replay_data::{paths, Store};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bumped only for changes the loader cannot infer. Adding an optional field
/// needs no bump (docs/schema.md).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub schema_version: u32,
    /// Creation time in unix ms, also the directory name.
    pub id: String,
    /// Informational only; never an engine input (ADR 0010).
    pub created_ms: i64,
    pub instrument: Instrument,
    pub range_from: Timestamp,
    pub range_to: Timestamp,
    pub balance: f64,
    /// Applied to market and stop fills when the provider has no real bid/ask
    /// (spec §5.3). Ignored for [`SpreadMode::Historical`] instruments.
    pub spread_points: f64,
    pub commission_per_unit: f64,
}

/// The account a session trades (spec §4): one per session, no multi-account
/// and no portfolio-level netting. Grouped rather than passed as loose floats
/// so a caller cannot silently swap the spread and the commission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Account {
    pub balance: f64,
    /// Spread in instrument points, applied to buys (see `fills::SideBar`).
    pub spread_points: f64,
    pub commission_per_unit: f64,
}

/// One line of `events.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoggedEvent {
    /// 1-based and dense.
    pub seq: u64,
    /// The cursor at which the user acted.
    pub cursor: Timestamp,
    #[serde(flatten)]
    pub event: Event,
}

/// What the user did. Each addition is a new variant, which older logs simply
/// never contain — that is what keeps the log forward-compatible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The cursor moved. `to` is where it landed, which is always the open
    /// time of a bar that exists.
    CursorSet {
        to: Timestamp,
    },
    /// A new order. Its id is this event's `seq`.
    OrderPlace {
        side: Side,
        kind: OrderKind,
        qty: f64,
        /// Trigger price; required for limit and stop.
        price: Option<f64>,
        sl: Option<f64>,
        tp: Option<f64>,
    },
    /// Set protection outright: `None` removes that leg. Targets a working
    /// order by id, or the open position when `order` is `None`. Moving to
    /// break-even is this event with `sl` equal to the entry price.
    OrderModify {
        order: Option<u64>,
        sl: Option<f64>,
        tp: Option<f64>,
    },
    OrderCancel {
        order: u64,
    },
    /// A journal note. `note` identifies it; writing the same id again edits
    /// it, which is how an append-only log expresses an edit (ADR 0011).
    /// `trade` optionally pins the note to a ledger row.
    NoteSet {
        note: u64,
        text: String,
        tags: Vec<String>,
        trade: Option<usize>,
    },
    /// Close the position at market. `qty` of `None` closes all of it.
    PositionClose {
        qty: Option<f64>,
    },
}

impl Session {
    /// Create a session and pin its candle data.
    ///
    /// `now_ms` is passed in rather than read from the clock so tests are
    /// reproducible; it only ever names the directory.
    pub fn create(
        root: &Path,
        instrument: &Instrument,
        range_from: Timestamp,
        range_to: Timestamp,
        account: Account,
        now_ms: i64,
    ) -> Result<Session> {
        if range_to <= range_from {
            return Err(Error::Data("session range ends before it starts".into()));
        }
        let session = Session {
            schema_version: SCHEMA_VERSION,
            id: now_ms.to_string(),
            created_ms: now_ms,
            instrument: instrument.clone(),
            range_from,
            range_to,
            balance: account.balance,
            // Negative costs would pay the trader to trade.
            spread_points: account.spread_points.max(0.0),
            commission_per_unit: account.commission_per_unit.max(0.0),
        };
        let dir = paths::session_dir(root, &session.id);
        std::fs::create_dir_all(&dir)?;

        // ADR 0012: copy the slice this session will replay. If the cache has
        // nothing, say so now rather than opening an empty chart.
        let store = Store::open()?;
        let source = paths::base_parquet(root, &instrument.provider, &instrument.symbol);
        let bars = store.bars(&source, range_from, range_to)?;
        if bars.is_empty() {
            return Err(Error::Data(format!(
                "no cached bars for {}/{} in that range; fetch them first",
                instrument.provider, instrument.symbol
            )));
        }
        store.upsert_bars(&Self::pinned_bars_path(&dir), &bars)?;

        std::fs::write(
            dir.join("session.json"),
            serde_json::to_string_pretty(&session)
                .map_err(|e| Error::Data(format!("encoding session: {e}")))?,
        )?;
        std::fs::write(dir.join("events.jsonl"), "")?;
        Ok(session)
    }

    pub fn load(root: &Path, id: &str) -> Result<Session> {
        let dir = paths::session_dir(root, id);
        let text = std::fs::read_to_string(dir.join("session.json"))?;
        let session: Session = serde_json::from_str(&text)
            .map_err(|e| Error::Data(format!("session {id} is unreadable: {e}")))?;
        if session.schema_version > SCHEMA_VERSION {
            return Err(Error::Data(format!(
                "session {id} was written by a newer version of the app \
                 (schema {} > {SCHEMA_VERSION}); upgrade to open it",
                session.schema_version
            )));
        }
        Ok(session)
    }

    /// Every session on disk, newest first.
    pub fn list(root: &Path) -> Result<Vec<Session>> {
        let dir = root.join("sessions");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                // A half-written session directory must not break the list.
                if let Ok(s) = Session::load(root, name) {
                    out.push(s);
                }
            }
        }
        out.sort_by_key(|s| std::cmp::Reverse(s.created_ms));
        Ok(out)
    }

    pub fn dir(&self, root: &Path) -> PathBuf {
        paths::session_dir(root, &self.id)
    }

    fn pinned_bars_path(dir: &Path) -> PathBuf {
        dir.join("1m.parquet")
    }

    /// The session's own copy of the base series (ADR 0012), which is the only
    /// file the replay ever reads.
    pub fn bars_path(&self, root: &Path) -> PathBuf {
        Self::pinned_bars_path(&self.dir(root))
    }

    pub fn events_path(&self, root: &Path) -> PathBuf {
        self.dir(root).join("events.jsonl")
    }

    /// Record an event, updating `log` and the file together.
    ///
    /// Consecutive cursor moves collapse into the latest one. Replaying a
    /// session walks every bar between two cursors regardless of how the user
    /// got there (docs/schema.md), so the intermediate positions carry no
    /// information — and writing one line per bar would turn a played-through
    /// session into tens of thousands of lines that say nothing.
    ///
    /// The file is rewritten through a temporary file and renamed, so a crash
    /// mid-write leaves the previous log intact rather than a truncated one.
    pub fn record(
        &self,
        root: &Path,
        log: &mut Vec<LoggedEvent>,
        cursor: Timestamp,
        event: Event,
    ) -> Result<()> {
        let collapses = matches!(event, Event::CursorSet { .. })
            && matches!(log.last().map(|e| &e.event), Some(Event::CursorSet { .. }));
        if collapses {
            log.pop();
        }
        log.push(LoggedEvent {
            seq: log.len() as u64 + 1,
            cursor,
            event,
        });

        let mut text = String::new();
        for e in log.iter() {
            text.push_str(
                &serde_json::to_string(e)
                    .map_err(|err| Error::Data(format!("encoding event: {err}")))?,
            );
            text.push('\n');
        }
        let path = self.events_path(root);
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn events(&self, root: &Path) -> Result<Vec<LoggedEvent>> {
        let path = self.events_path(root);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path)?;
        let mut out = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let event: LoggedEvent = serde_json::from_str(line).map_err(|e| {
                Error::Data(format!(
                    "{}: line {} is unreadable: {e}",
                    path.display(),
                    i + 1
                ))
            })?;
            out.push(event);
        }
        Ok(out)
    }

    /// Where a resumed session puts the cursor: exactly where it was left.
    /// A session with no cursor events has not moved from its first bar.
    pub fn resume_cursor(&self, root: &Path) -> Result<Option<Timestamp>> {
        Ok(self.events(root)?.iter().rev().find_map(|e| match e.event {
            Event::CursorSet { to } => Some(to),
            _ => None,
        }))
    }
}
