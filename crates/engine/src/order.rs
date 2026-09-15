//! Orders, positions and the trade ledger.
//!
//! The ledger is the source of truth; the position is folded from it and never
//! stored as independently mutable state (ADR 0009). Nothing in here mutates
//! anything: the simulator rebuilds all of it from the event log and the bars,
//! which is what makes stepping backward exact (invariant I5).

use replay_core::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }

    /// +1 for a long, -1 for a short; the sign of the P&L per point.
    pub fn sign(self) -> f64 {
        match self {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    /// Fills now, at the price the trader can actually see.
    Market,
    /// Rests below (buy) or above (sell) the market.
    Limit,
    /// Rests above (buy) or below (sell) the market.
    Stop,
}

/// A resting order. `id` is the `seq` of the event that placed it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub side: Side,
    pub kind: OrderKind,
    pub qty: f64,
    /// Trigger price. Required for limit and stop, ignored for market.
    pub price: Option<f64>,
    pub sl: Option<f64>,
    pub tp: Option<f64>,
    /// Cursor at which the user placed it.
    pub placed: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub side: Side,
    pub qty: f64,
    pub avg_entry: f64,
    pub sl: Option<f64>,
    pub tp: Option<f64>,
    pub opened: Timestamp,
}

impl Position {
    /// Profit in account currency if closed at `price`.
    pub fn unrealised(&self, price: f64, multiplier: f64) -> f64 {
        (price - self.avg_entry) * self.side.sign() * self.qty * multiplier
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Entry,
    Exit,
}

/// Why a fill happened. Shown in the trade log so a trader can audit it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Market,
    Limit,
    Stop,
    Sl,
    Tp,
    /// Manual close, full or partial.
    Close,
}

/// Whether the fill price rests on an assumption rather than on data.
///
/// Invariant I3: when one bar touches both stop-loss and take-profit, OHLC alone
/// cannot say which came first. Finer data settles it when we have it;
/// otherwise the pessimistic answer is used and **flagged here**, so the UI can
/// mark every trade that depended on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assumption {
    /// The bar settled it on its own, or ticks did.
    None,
    /// The bar hit both exits; stop-loss was assumed to come first.
    SlFirst,
    /// Ticks were consulted and resolved the order of the two touches.
    ResolvedByTicks,
}

/// One row of the ledger, appended on every fill (ADR 0009).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub seq: usize,
    /// Open time of the bar the fill happened in.
    pub cursor: Timestamp,
    /// The order that caused the fill, or 0 for a position-level exit
    /// (stop-loss, take-profit or a manual close).
    pub order: u64,
    pub side: Side,
    pub qty: f64,
    pub price: f64,
    pub role: Role,
    pub reason: Reason,
    pub assumption: Assumption,
    pub commission: f64,
    /// Net effect on the balance: `-commission` on an entry, gross P&L less
    /// commission on an exit. The balance is the starting balance plus the sum
    /// of these, which makes the ledger auditable by addition alone.
    pub realised: f64,
    /// Distance from entry price to the stop that was set when this position
    /// opened, per unit. `None` when the trade was taken without a stop, which
    /// is also why an R-multiple is not always definable.
    pub risk_per_unit: Option<f64>,
}

/// A completed entry-to-exit round trip. A view over the
/// ledger, never a second table.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundTrip {
    pub opened: Timestamp,
    pub closed: Timestamp,
    pub side: Side,
    pub qty: f64,
    pub entry: f64,
    pub exit: f64,
    /// Net of commission on both legs.
    pub realised: f64,
    /// Profit divided by the risk taken at entry, when a stop was set.
    pub r_multiple: Option<f64>,
    pub assumption: Assumption,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_gains_when_price_rises_and_a_short_when_it_falls() {
        let long = Position {
            side: Side::Buy,
            qty: 2.0,
            avg_entry: 100.0,
            sl: None,
            tp: None,
            opened: Timestamp(0),
        };
        assert_eq!(long.unrealised(105.0, 1.0), 10.0);
        assert_eq!(long.unrealised(95.0, 1.0), -10.0);

        let short = Position {
            side: Side::Sell,
            ..long
        };
        assert_eq!(short.unrealised(105.0, 1.0), -10.0);
        assert_eq!(short.unrealised(95.0, 1.0), 10.0);
    }

    #[test]
    fn the_multiplier_scales_pnl_for_futures_style_instruments() {
        let nq = Position {
            side: Side::Buy,
            qty: 1.0,
            avg_entry: 17_000.0,
            sl: None,
            tp: None,
            opened: Timestamp(0),
        };
        // NQ is $20 per index point.
        assert_eq!(nq.unrealised(17_010.0, 20.0), 200.0);
    }
}
