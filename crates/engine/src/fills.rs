//! Where a fill happens, and at what price.
//!
//! Pure functions over one bar, so every rule here is testable without a
//! session, a database or a clock. Two rules matter most:
//!
//! * **Gaps fill at the open, not at the trigger.** If a bar opens beyond a
//!   stop, the trader did not get the stop price — they got the open. Filling
//!   at the trigger would manufacture money that never existed.
//! * **A bar that touches both exits is ambiguous** (spec §5.3). OHLC cannot
//!   say which came first. Ticks settle it when we have them; otherwise the
//!   stop-loss is assumed to have come first and the trade is flagged.

use crate::order::{Assumption, OrderKind, Side};
use replay_core::{Bar, Tick};

/// A bar as one side of the book sees it.
///
/// Base bars carry bid (Dukascopy) or trade (Binance) prices, so a buy executes
/// `spread` above them and a sell at them. Shifting the whole bar keeps every
/// comparison below in one consistent set of units.
#[derive(Clone, Copy, Debug)]
pub struct SideBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

impl SideBar {
    pub fn of(bar: &Bar, side: Side, spread: f64) -> SideBar {
        let shift = match side {
            Side::Buy => spread,
            Side::Sell => 0.0,
        };
        SideBar {
            open: bar.open + shift,
            high: bar.high + shift,
            low: bar.low + shift,
            close: bar.close + shift,
        }
    }
}

/// The price a resting order fills at within `bar`, or `None` if untouched.
pub fn resting_fill(kind: OrderKind, side: Side, trigger: f64, bar: SideBar) -> Option<f64> {
    match (kind, side) {
        // A buy limit rests below the market and fills on a dip to it. If the
        // bar opened below it, the trader got the better price.
        (OrderKind::Limit, Side::Buy) => (bar.low <= trigger).then(|| trigger.min(bar.open)),
        (OrderKind::Limit, Side::Sell) => (bar.high >= trigger).then(|| trigger.max(bar.open)),
        // A buy stop rests above the market. A bar that opens above it gapped
        // through: the fill is the open, which is worse than the trigger.
        (OrderKind::Stop, Side::Buy) => (bar.high >= trigger).then(|| trigger.max(bar.open)),
        (OrderKind::Stop, Side::Sell) => (bar.low <= trigger).then(|| trigger.min(bar.open)),
        // A market order never rests; it fills where it is placed.
        (OrderKind::Market, _) => None,
    }
}

/// Which exit a bar hit, once both stop-loss and take-profit are considered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exit {
    Sl { price: f64, assumption: Assumption },
    Tp { price: f64, assumption: Assumption },
}

/// Decide the exit for a position whose closing side is `exit_side`.
///
/// `sl` and `tp` are absolute prices. `ticks` are the quotes inside this bar,
/// ascending, when the session has them pinned (ADR 0013).
pub fn exit_for(
    exit_side: Side,
    sl: Option<f64>,
    tp: Option<f64>,
    bar: SideBar,
    ticks: Option<&[Tick]>,
    spread: f64,
) -> Option<Exit> {
    // A long exits by selling: its stop is a sell stop, its target a sell limit.
    let sl_hit = sl.and_then(|p| resting_fill(OrderKind::Stop, exit_side, p, bar));
    let tp_hit = tp.and_then(|p| resting_fill(OrderKind::Limit, exit_side, p, bar));

    match (sl_hit, tp_hit) {
        (None, None) => None,
        (Some(price), None) => Some(Exit::Sl {
            price,
            assumption: Assumption::None,
        }),
        (None, Some(price)) => Some(Exit::Tp {
            price,
            assumption: Assumption::None,
        }),
        (Some(sl_price), Some(tp_price)) => {
            // Both touched inside one bar. This is the ambiguous case.
            match ticks.and_then(|t| first_touch(exit_side, sl?, tp?, t, spread)) {
                Some(true) => Some(Exit::Sl {
                    price: sl_price,
                    assumption: Assumption::ResolvedByTicks,
                }),
                Some(false) => Some(Exit::Tp {
                    price: tp_price,
                    assumption: Assumption::ResolvedByTicks,
                }),
                // No finer data: assume the worse outcome and say so.
                None => Some(Exit::Sl {
                    price: sl_price,
                    assumption: Assumption::SlFirst,
                }),
            }
        }
    }
}

/// Walk the ticks in order and report whether the stop was touched before the
/// target. `None` means the ticks never touched either, so they cannot settle
/// it and the caller must fall back to the assumption.
fn first_touch(exit_side: Side, sl: f64, tp: f64, ticks: &[Tick], spread: f64) -> Option<bool> {
    for tick in ticks {
        // Exiting by selling hits the bid; exiting by buying lifts the ask.
        let price = match exit_side {
            Side::Sell => tick.bid,
            Side::Buy => tick.ask.max(tick.bid + spread),
        };
        let hit_sl = match exit_side {
            Side::Sell => price <= sl,
            Side::Buy => price >= sl,
        };
        let hit_tp = match exit_side {
            Side::Sell => price >= tp,
            Side::Buy => price <= tp,
        };
        if hit_sl {
            return Some(true);
        }
        if hit_tp {
            return Some(false);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use replay_core::Timestamp;

    fn bar(open: f64, high: f64, low: f64, close: f64) -> SideBar {
        SideBar {
            open,
            high,
            low,
            close,
        }
    }

    #[test]
    fn a_buy_limit_below_the_bar_is_not_touched() {
        assert_eq!(
            resting_fill(
                OrderKind::Limit,
                Side::Buy,
                90.0,
                bar(100.0, 102.0, 99.0, 101.0)
            ),
            None
        );
    }

    #[test]
    fn a_buy_limit_inside_the_bar_fills_at_its_price() {
        assert_eq!(
            resting_fill(
                OrderKind::Limit,
                Side::Buy,
                99.5,
                bar(100.0, 102.0, 99.0, 101.0)
            ),
            Some(99.5)
        );
    }

    #[test]
    fn a_limit_that_gaps_past_fills_at_the_better_open_not_the_trigger() {
        // Opened at 95 with a buy limit at 99.5: the trader got 95.
        assert_eq!(
            resting_fill(
                OrderKind::Limit,
                Side::Buy,
                99.5,
                bar(95.0, 96.0, 94.0, 95.5)
            ),
            Some(95.0)
        );
    }

    /// Spec §6: "gap-open through a stop".
    #[test]
    fn a_stop_that_gaps_past_fills_at_the_worse_open_not_the_trigger() {
        // Long's stop at 99, bar opens at 90: the fill is 90, not 99.
        assert_eq!(
            resting_fill(
                OrderKind::Stop,
                Side::Sell,
                99.0,
                bar(90.0, 91.0, 89.0, 90.5)
            ),
            Some(90.0)
        );
        // A buy stop at 101 on a bar that opens at 110 fills at 110.
        assert_eq!(
            resting_fill(
                OrderKind::Stop,
                Side::Buy,
                101.0,
                bar(110.0, 111.0, 109.0, 110.5)
            ),
            Some(110.0)
        );
    }

    #[test]
    fn the_spread_lifts_every_buy_price_and_leaves_sells_alone() {
        let raw = Bar {
            ts: Timestamp(0),
            open: 100.0,
            high: 102.0,
            low: 99.0,
            close: 101.0,
            volume: 1.0,
        };
        let buy = SideBar::of(&raw, Side::Buy, 0.5);
        let sell = SideBar::of(&raw, Side::Sell, 0.5);
        assert_eq!((buy.open, buy.high, buy.low), (100.5, 102.5, 99.5));
        assert_eq!((sell.open, sell.high, sell.low), (100.0, 102.0, 99.0));
    }

    #[test]
    fn one_exit_touched_needs_no_assumption() {
        let b = bar(100.0, 102.0, 99.0, 101.0);
        assert_eq!(
            exit_for(Side::Sell, Some(99.5), Some(110.0), b, None, 0.0),
            Some(Exit::Sl {
                price: 99.5,
                assumption: Assumption::None
            })
        );
        assert_eq!(
            exit_for(Side::Sell, Some(90.0), Some(101.5), b, None, 0.0),
            Some(Exit::Tp {
                price: 101.5,
                assumption: Assumption::None
            })
        );
        assert_eq!(
            exit_for(Side::Sell, Some(90.0), Some(110.0), b, None, 0.0),
            None
        );
    }

    /// Spec §5.3: never silently pick the favourable outcome.
    #[test]
    fn a_bar_touching_both_exits_assumes_the_stop_and_flags_it() {
        let b = bar(100.0, 102.0, 99.0, 101.0);
        let exit = exit_for(Side::Sell, Some(99.5), Some(101.5), b, None, 0.0).unwrap();
        assert_eq!(
            exit,
            Exit::Sl {
                price: 99.5,
                assumption: Assumption::SlFirst
            },
            "the pessimistic outcome must win, and be marked"
        );
    }

    #[test]
    fn ticks_settle_an_ambiguous_bar_in_either_direction() {
        let b = bar(100.0, 102.0, 99.0, 101.0);
        let up_first = [
            Tick {
                ts: Timestamp(0),
                bid: 100.2,
                ask: 100.2,
            },
            Tick {
                ts: Timestamp(1),
                bid: 101.6,
                ask: 101.6,
            }, // target
            Tick {
                ts: Timestamp(2),
                bid: 99.4,
                ask: 99.4,
            }, // stop, but later
        ];
        assert_eq!(
            exit_for(Side::Sell, Some(99.5), Some(101.5), b, Some(&up_first), 0.0),
            Some(Exit::Tp {
                price: 101.5,
                assumption: Assumption::ResolvedByTicks
            })
        );

        let down_first = [
            Tick {
                ts: Timestamp(0),
                bid: 99.4,
                ask: 99.4,
            },
            Tick {
                ts: Timestamp(1),
                bid: 101.6,
                ask: 101.6,
            },
        ];
        assert_eq!(
            exit_for(
                Side::Sell,
                Some(99.5),
                Some(101.5),
                b,
                Some(&down_first),
                0.0
            ),
            Some(Exit::Sl {
                price: 99.5,
                assumption: Assumption::ResolvedByTicks
            })
        );
    }

    #[test]
    fn ticks_that_settle_nothing_fall_back_to_the_stop_assumption() {
        let b = bar(100.0, 102.0, 99.0, 101.0);
        let useless = [Tick {
            ts: Timestamp(0),
            bid: 100.5,
            ask: 100.5,
        }];
        assert_eq!(
            exit_for(Side::Sell, Some(99.5), Some(101.5), b, Some(&useless), 0.0),
            Some(Exit::Sl {
                price: 99.5,
                assumption: Assumption::SlFirst
            })
        );
    }
}
