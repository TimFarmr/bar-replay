//! The simulator: bars plus the event log in, trade ledger out.
//!
//! [`simulate`] is a **pure function of the cursor**. It never mutates
//! long-lived state, so moving the cursor backwards is not an undo operation
//! that has to be written and maintained — it is the same computation over
//! fewer bars. That is what makes spec §5.5 hold by construction: there is no
//! second copy of state that could be left behind as a ghost fill.
//!
//! Within one bar the order is deliberate:
//! 1. resting orders trigger,
//! 2. the open position's stop-loss / take-profit are checked,
//! 3. only then are the user's actions at this cursor applied.
//!
//! Step 3 comes last because the trader acts having already *seen* this bar.
//! Letting an order placed at bar T fill on bar T's earlier range would be
//! trading on information they only had in hindsight.

use crate::fills::{exit_for, resting_fill, Exit, SideBar};
use crate::order::{Assumption, Order, OrderKind, Position, Reason, Role, Side, Trade};
use crate::session::{Event, LoggedEvent};
use replay_core::{Bar, Tick, Timestamp};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct SimConfig {
    pub starting_balance: f64,
    /// Absolute price units, already converted from points.
    pub spread: f64,
    pub commission_per_unit: f64,
    /// Account currency per 1.0 of price move per unit.
    pub multiplier: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SimState {
    pub balance: f64,
    pub working: Vec<Order>,
    pub position: Option<Position>,
    pub trades: Vec<Trade>,
}

/// Quotes finer than the base bar, used only to settle ambiguous bars
/// (ADR 0013). Sessions that have none simply answer `None`.
pub trait TickSource {
    fn ticks_in(&self, minute: Timestamp) -> Option<Vec<Tick>>;
}

/// The provider has no ticks, or none have been pinned yet.
pub struct NoTicks;

impl TickSource for NoTicks {
    fn ticks_in(&self, _: Timestamp) -> Option<Vec<Tick>> {
        None
    }
}

/// Rebuild the whole trading state as of `cursor`.
///
/// `bars` must be ascending; `events` must be in `seq` order. Bars and events
/// after `cursor` are ignored, which is the no-lookahead rule (spec §5.1)
/// applied to simulation rather than to drawing.
pub fn simulate(
    cfg: SimConfig,
    bars: &[Bar],
    events: &[LoggedEvent],
    cursor: Timestamp,
    ticks: &dyn TickSource,
) -> SimState {
    let mut st = SimState {
        balance: cfg.starting_balance,
        ..Default::default()
    };
    let mut next_event = 0;

    for bar in bars.iter().take_while(|b| b.ts <= cursor) {
        trigger_resting(&mut st, bar, &cfg);
        check_exits(&mut st, bar, &cfg, ticks);

        while next_event < events.len() && events[next_event].cursor <= bar.ts {
            let logged = &events[next_event];
            next_event += 1;
            apply(&mut st, logged, bar, &cfg);
        }
    }
    st
}

/// Resting limit and stop orders that this bar reaches.
fn trigger_resting(st: &mut SimState, bar: &Bar, cfg: &SimConfig) {
    let mut filled: Vec<(Order, f64)> = Vec::new();
    st.working.retain(|order| {
        let view = SideBar::of(bar, order.side, cfg.spread);
        match order
            .price
            .and_then(|p| resting_fill(order.kind, order.side, p, view))
        {
            Some(price) => {
                filled.push((*order, price));
                false
            }
            None => true,
        }
    });
    for (order, price) in filled {
        let reason = match order.kind {
            OrderKind::Limit => Reason::Limit,
            _ => Reason::Stop,
        };
        execute(
            st,
            cfg,
            bar.ts,
            order.id,
            order.side,
            order.qty,
            price,
            reason,
            Assumption::None,
        );
        // A filled entry carries its protection onto the position.
        if let Some(pos) = st.position.as_mut() {
            if order.sl.is_some() || order.tp.is_some() {
                pos.sl = order.sl;
                pos.tp = order.tp;
            }
        }
        note_entry_risk(st, order.sl);
    }
}

/// The open position's stop-loss and take-profit against this bar.
fn check_exits(st: &mut SimState, bar: &Bar, cfg: &SimConfig, ticks: &dyn TickSource) {
    let Some(pos) = st.position else { return };
    if pos.sl.is_none() && pos.tp.is_none() {
        return;
    }
    let exit_side = pos.side.opposite();
    let view = SideBar::of(bar, exit_side, cfg.spread);

    // Only pay for ticks when the bar is genuinely ambiguous.
    let both = pos.sl.is_some() && pos.tp.is_some();
    let ticks = if both { ticks.ticks_in(bar.ts) } else { None };

    let Some(exit) = exit_for(
        exit_side,
        pos.sl,
        pos.tp,
        view,
        ticks.as_deref(),
        cfg.spread,
    ) else {
        return;
    };
    let (price, reason, assumption) = match exit {
        Exit::Sl { price, assumption } => (price, Reason::Sl, assumption),
        Exit::Tp { price, assumption } => (price, Reason::Tp, assumption),
    };
    execute(
        st, cfg, bar.ts, 0, exit_side, pos.qty, price, reason, assumption,
    );
}

fn apply(st: &mut SimState, logged: &LoggedEvent, bar: &Bar, cfg: &SimConfig) {
    match &logged.event {
        // Cursor moves do not affect fills: the engine walks every bar between
        // two cursors regardless of how the user got there. Notes are the
        // trader writing in the margin; they change nothing that fills.
        // Matched by name rather than with a wildcard so a new event type has
        // to be considered here rather than silently ignored.
        Event::CursorSet { .. } | Event::NoteSet { .. } => {}

        Event::OrderPlace {
            side,
            kind,
            qty,
            price,
            sl,
            tp,
        } => {
            if *qty <= 0.0 {
                return;
            }
            if *kind == OrderKind::Market {
                let view = SideBar::of(bar, *side, cfg.spread);
                execute(
                    st,
                    cfg,
                    bar.ts,
                    logged.seq,
                    *side,
                    *qty,
                    view.close,
                    Reason::Market,
                    Assumption::None,
                );
                // Only attach protection the order actually carried. Adding to
                // a position with a bare market order must never silently
                // remove the stop already guarding it — the one way to clear a
                // stop is to ask for it, with OrderModify.
                if let Some(pos) = st.position.as_mut() {
                    if sl.is_some() || tp.is_some() {
                        pos.sl = *sl;
                        pos.tp = *tp;
                    }
                }
                note_entry_risk(st, *sl);
            } else if price.is_some() {
                st.working.push(Order {
                    id: logged.seq,
                    side: *side,
                    kind: *kind,
                    qty: *qty,
                    price: *price,
                    sl: *sl,
                    tp: *tp,
                    placed: logged.cursor,
                });
            }
        }

        Event::OrderModify { order, sl, tp } => match order {
            Some(id) => {
                if let Some(o) = st.working.iter_mut().find(|o| o.id == *id) {
                    o.sl = *sl;
                    o.tp = *tp;
                }
            }
            None => {
                if let Some(pos) = st.position.as_mut() {
                    pos.sl = *sl;
                    pos.tp = *tp;
                }
            }
        },

        Event::OrderCancel { order } => st.working.retain(|o| o.id != *order),

        Event::PositionClose { qty } => {
            if let Some(pos) = st.position {
                let amount = qty.unwrap_or(pos.qty).min(pos.qty);
                if amount > 0.0 {
                    let side = pos.side.opposite();
                    let view = SideBar::of(bar, side, cfg.spread);
                    execute(
                        st,
                        cfg,
                        bar.ts,
                        0,
                        side,
                        amount,
                        view.close,
                        Reason::Close,
                        Assumption::None,
                    );
                }
            }
        }
    }
}

/// Apply one fill to the ledger and the position.
///
/// Opposite-side quantity closes the position first and only opens a new one
/// with whatever is left over, so there is never more than one position
/// (ADR 0008).
#[allow(clippy::too_many_arguments)]
fn execute(
    st: &mut SimState,
    cfg: &SimConfig,
    ts: Timestamp,
    order: u64,
    side: Side,
    qty: f64,
    price: f64,
    reason: Reason,
    assumption: Assumption,
) {
    let mut remaining = qty;

    if let Some(pos) = st.position {
        if pos.side != side {
            let closing = remaining.min(pos.qty);
            let gross = (price - pos.avg_entry) * pos.side.sign() * closing * cfg.multiplier;
            let commission = cfg.commission_per_unit * closing;
            let realised = gross - commission;
            st.balance += realised;
            push(
                st,
                ts,
                order,
                side,
                closing,
                price,
                Role::Exit,
                reason,
                assumption,
                commission,
                realised,
            );

            remaining -= closing;
            st.position = if pos.qty - closing > 0.0 {
                Some(Position {
                    qty: pos.qty - closing,
                    ..pos
                })
            } else {
                None
            };
            if remaining <= 0.0 {
                return;
            }
        }
    }

    // Opening, or adding to, a position in `side`.
    let commission = cfg.commission_per_unit * remaining;
    st.balance -= commission;
    push(
        st,
        ts,
        order,
        side,
        remaining,
        price,
        Role::Entry,
        reason,
        assumption,
        commission,
        -commission,
    );

    st.position = Some(match st.position {
        Some(pos) => {
            let total = pos.qty + remaining;
            Position {
                avg_entry: (pos.avg_entry * pos.qty + price * remaining) / total,
                qty: total,
                ..pos
            }
        }
        None => Position {
            side,
            qty: remaining,
            avg_entry: price,
            sl: None,
            tp: None,
            opened: ts,
        },
    });
}

#[allow(clippy::too_many_arguments)]
fn push(
    st: &mut SimState,
    cursor: Timestamp,
    order: u64,
    side: Side,
    qty: f64,
    price: f64,
    role: Role,
    reason: Reason,
    assumption: Assumption,
    commission: f64,
    realised: f64,
) {
    st.trades.push(Trade {
        seq: st.trades.len() + 1,
        cursor,
        order,
        side,
        qty,
        price,
        role,
        reason,
        assumption,
        commission,
        realised,
        risk_per_unit: None,
    });
}

/// Record what the trader was risking per unit when the position opened, which
/// is what an R-multiple is measured against (M4). Called once the entry's
/// protection is known, since a market order carries its stop in the same
/// event that fills it.
fn note_entry_risk(st: &mut SimState, sl: Option<f64>) {
    let Some(sl) = sl else { return };
    if let Some(t) = st.trades.last_mut() {
        if t.role == Role::Entry {
            t.risk_per_unit = Some((t.price - sl).abs());
        }
    }
}
