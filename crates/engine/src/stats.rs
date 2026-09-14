//! Round trips, equity curve and summary statistics (M4).
//!
//! Everything here is a **view over the trade ledger** (ADR 0009), computed on
//! demand from [`Trade`] rows and never stored as a second table that could
//! drift from them. Every function is pure: same ledger in, same numbers out,
//! which is what lets the review screen be rebuilt from the event log alone.
//!
//! Two rules shape the arithmetic below:
//!
//! * A statistic is never allowed to become `NaN` or infinity. Both serialise
//!   as `null` or as a nonsense number over IPC and would corrupt the stats
//!   panel; an empty sample is reported as `0.0` or `None` instead.
//! * An R-multiple is only defined when the trade had a stop (spec §4), so it
//!   is an `Option` all the way to the UI rather than a silent zero.

use crate::order::{Assumption, Role, RoundTrip, Side, Trade};
use replay_core::Timestamp;
use serde::Serialize;
use std::collections::VecDeque;

/// An entry fill that is still waiting to be closed.
///
/// Commission is carried per unit because a single entry can be closed by
/// several exits, and each resulting round trip should only bear the share of
/// the entry cost that belongs to the quantity it closed.
struct Lot {
    opened: Timestamp,
    side: Side,
    price: f64,
    /// Quantity not yet closed.
    qty: f64,
    risk_per_unit: Option<f64>,
    commission_per_unit: f64,
}

/// Pair entry fills with the exits that closed them, first in first out.
///
/// FIFO rather than average-price matching because the trade log shows the
/// trader the individual fills they made: a scale-in closed at a profit should
/// report the first lot's result against the first lot's price, not against a
/// blended price they never traded at.
///
/// A position still open when the ledger ends produces nothing — it is not yet
/// a round trip, and guessing at its outcome with the last price would mix a
/// realised statistic with an unrealised one.
pub fn round_trips(trades: &[Trade], multiplier: f64) -> Vec<RoundTrip> {
    let mut open: VecDeque<Lot> = VecDeque::new();
    let mut trips = Vec::new();

    for t in trades {
        // Per-unit arithmetic is undefined for a zero-quantity fill, and the
        // simulator never writes one; skipping is cheaper than propagating a
        // `NaN` into every statistic downstream.
        if t.qty <= 0.0 {
            continue;
        }
        match t.role {
            Role::Entry => open.push_back(Lot {
                opened: t.cursor,
                side: t.side,
                price: t.price,
                qty: t.qty,
                risk_per_unit: t.risk_per_unit,
                commission_per_unit: t.commission / t.qty,
            }),
            Role::Exit => {
                let exit_commission_per_unit = t.commission / t.qty;
                let mut left = t.qty;
                while left > 0.0 {
                    // More closed than was ever opened. The simulator cannot
                    // produce this, but a hand-edited ledger could, and the
                    // review screen should show what it can pair rather than
                    // panic.
                    let Some(lot) = open.front_mut() else { break };

                    let qty = left.min(lot.qty);
                    // Recomputed from the two prices rather than pro-rated out
                    // of the exit's `realised`: after a scale-in each lot has
                    // its own entry, and splitting the exit's P&L evenly per
                    // unit would credit the profit to the wrong lot. The parts
                    // still sum to the exit's own figure, because the position's
                    // average entry is the weighted mean of these lots.
                    let gross = (t.price - lot.price) * lot.side.sign() * qty * multiplier;
                    let realised =
                        gross - qty * (lot.commission_per_unit + exit_commission_per_unit);

                    trips.push(RoundTrip {
                        opened: lot.opened,
                        closed: t.cursor,
                        side: lot.side,
                        qty,
                        entry: lot.price,
                        exit: t.price,
                        realised,
                        r_multiple: r_multiple(realised, lot.risk_per_unit, qty, multiplier),
                        // The exit's assumption, so a trade that depended on the
                        // pessimistic stop-first guess (spec §5.3) stays flagged
                        // everywhere it is shown.
                        assumption: t.assumption,
                    });

                    lot.qty -= qty;
                    left -= qty;
                    if lot.qty <= 0.0 {
                        open.pop_front();
                    }
                }
            }
        }
    }
    trips
}

/// Profit expressed in units of the risk taken at entry.
///
/// `None` when there was no stop, and equally when the stop sat on the entry
/// price: both mean "no risk was defined", and an infinite R would read as a
/// spectacular trade rather than as missing information.
fn r_multiple(realised: f64, risk_per_unit: Option<f64>, qty: f64, multiplier: f64) -> Option<f64> {
    let risk = risk_per_unit? * qty * multiplier;
    (risk != 0.0).then(|| realised / risk)
}

/// One point on the balance curve, after a fill.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Equity {
    pub cursor: Timestamp,
    pub balance: f64,
}

/// The running balance, one point per fill.
///
/// The curve opens with the starting balance at the first fill's cursor so the
/// first trade is drawn as a move away from the account's starting point
/// rather than as the account springing into existence already in profit.
/// An empty ledger has no cursor to anchor that point to, so it has no curve.
pub fn equity_curve(starting_balance: f64, trades: &[Trade]) -> Vec<Equity> {
    let Some(first) = trades.first() else {
        return Vec::new();
    };
    let mut balance = starting_balance;
    let mut curve = vec![Equity {
        cursor: first.cursor,
        balance,
    }];
    for t in trades {
        // Summing `realised` is the same addition the simulator does to the
        // balance, which keeps the curve and the ledger auditable against each
        // other by hand.
        balance += t.realised;
        curve.push(Equity {
            cursor: t.cursor,
            balance,
        });
    }
    curve
}

/// The performance figures shown on the review screen.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Round trips, not ledger rows: a scale-in is several trades here.
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    /// 0..1, not a percentage; formatting is the UI's business.
    pub win_rate: f64,
    pub net: f64,
    pub gross_profit: f64,
    /// Positive, so the two gross figures read as magnitudes side by side.
    pub gross_loss: f64,
    /// `None` when nothing was lost. A strategy with no losing trade has no
    /// meaningful ratio, and reporting infinity would flatter it.
    pub profit_factor: Option<f64>,
    pub average_win: f64,
    /// Positive, like `gross_loss`.
    pub average_loss: f64,
    /// Expected result per round trip.
    pub expectancy: f64,
    /// Largest peak-to-trough fall of the balance, as a positive number.
    pub max_drawdown: f64,
    /// Mean of the defined R-multiples. `None` when no trade had a stop;
    /// trades without one are left out rather than counted as zero R, which
    /// would drag the average toward a number nobody risked.
    pub average_r: Option<f64>,
    /// Round trips whose exit rested on the pessimistic stop-first assumption
    /// (spec §5.3). Surfaced as a count so a trader can judge how much of the
    /// result depended on a guess.
    pub flagged_trades: usize,
}

pub fn summary(starting_balance: f64, trips: &[RoundTrip]) -> Summary {
    let mut wins = 0;
    let mut losses = 0;
    let mut gross_profit = 0.0;
    let mut gross_loss = 0.0;
    let mut r_total = 0.0;
    let mut r_count = 0;
    let mut flagged_trades = 0;

    // Drawdown is measured on the balance after each round trip, which is the
    // same sequence the equity curve draws.
    let mut balance = starting_balance;
    let mut peak = starting_balance;
    let mut max_drawdown = 0.0f64;

    for t in trips {
        // A scratch trade is neither a win nor a loss, so `wins + losses` can
        // be short of `trades`. That is deliberate: counting a break-even as a
        // win would inflate the hit rate.
        if t.realised > 0.0 {
            wins += 1;
            gross_profit += t.realised;
        } else if t.realised < 0.0 {
            losses += 1;
            gross_loss -= t.realised;
        }
        if let Some(r) = t.r_multiple {
            r_total += r;
            r_count += 1;
        }
        if t.assumption == Assumption::SlFirst {
            flagged_trades += 1;
        }

        balance += t.realised;
        peak = peak.max(balance);
        max_drawdown = max_drawdown.max(peak - balance);
    }

    let net = gross_profit - gross_loss;
    Summary {
        trades: trips.len(),
        wins,
        losses,
        win_rate: mean(wins as f64, trips.len()),
        net,
        gross_profit,
        gross_loss,
        profit_factor: (gross_loss > 0.0).then(|| gross_profit / gross_loss),
        average_win: mean(gross_profit, wins),
        average_loss: mean(gross_loss, losses),
        expectancy: mean(net, trips.len()),
        max_drawdown,
        average_r: (r_count > 0).then(|| r_total / r_count as f64),
        flagged_trades,
    }
}

/// `total / count`, or zero for an empty sample.
///
/// Zero rather than `NaN`: an account with no trades has an average result of
/// nothing, and `NaN` would poison every comparison the UI makes on the value.
fn mean(total: f64, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}
