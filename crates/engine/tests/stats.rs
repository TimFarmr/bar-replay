//! Statistics tests.
//!
//! Ledgers here are written by hand rather than produced by the simulator. The
//! pairing rules have to hold for any ledger shape — partial closes, scale-ins,
//! a position left open — and building each shape out of bars and events would
//! test the simulator's fill logic all over again instead of the arithmetic
//! under test.

use replay_core::Timestamp;
use replay_engine::order::{Assumption, Reason, Role, RoundTrip, Side, Trade};
use replay_engine::stats::{equity_curve, round_trips, summary};

fn ts(minute: i64) -> Timestamp {
    Timestamp(minute * Timestamp::MINUTE)
}

/// An entry fill. `realised` on an entry is minus the commission, exactly as
/// the simulator writes it.
fn entry(seq: usize, minute: i64, side: Side, qty: f64, price: f64, commission: f64) -> Trade {
    Trade {
        seq,
        cursor: ts(minute),
        order: seq as u64,
        side,
        qty,
        price,
        role: Role::Entry,
        reason: Reason::Market,
        assumption: Assumption::None,
        commission,
        realised: -commission,
        risk_per_unit: None,
    }
}

fn exit(
    seq: usize,
    minute: i64,
    side: Side,
    qty: f64,
    price: f64,
    realised: f64,
    commission: f64,
) -> Trade {
    Trade {
        seq,
        cursor: ts(minute),
        order: 0,
        side,
        qty,
        price,
        role: Role::Exit,
        reason: Reason::Close,
        assumption: Assumption::None,
        commission,
        realised,
        risk_per_unit: None,
    }
}

fn approx(got: f64, want: f64) {
    assert!((got - want).abs() < 1e-9, "got {got}, want {want}");
}

/// A round trip with only the fields a summary looks at.
fn trip(realised: f64, r_multiple: Option<f64>, assumption: Assumption) -> RoundTrip {
    RoundTrip {
        opened: ts(0),
        closed: ts(1),
        side: Side::Buy,
        qty: 1.0,
        entry: 100.0,
        exit: 100.0,
        realised,
        r_multiple,
        assumption,
    }
}

#[test]
fn a_win_and_a_loss_pair_into_round_trips_with_their_r_multiples() {
    let won = [
        Trade {
            risk_per_unit: Some(2.0),
            ..entry(1, 0, Side::Buy, 1.0, 100.0, 0.0)
        },
        exit(2, 5, Side::Sell, 1.0, 106.0, 6.0, 0.0),
    ];
    let trips = round_trips(&won, 1.0);
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].opened, ts(0));
    assert_eq!(trips[0].closed, ts(5));
    assert_eq!(trips[0].side, Side::Buy);
    assert_eq!(trips[0].entry, 100.0);
    assert_eq!(trips[0].exit, 106.0);
    approx(trips[0].realised, 6.0);
    // Risked 2 a unit, made 6: three times the risk.
    approx(trips[0].r_multiple.expect("a stop was set"), 3.0);

    let lost = [
        Trade {
            risk_per_unit: Some(2.0),
            ..entry(1, 0, Side::Sell, 1.0, 100.0, 0.0)
        },
        exit(2, 5, Side::Buy, 1.0, 102.0, -2.0, 0.0),
    ];
    let trips = round_trips(&lost, 1.0);
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].side, Side::Sell);
    approx(trips[0].realised, -2.0);
    approx(trips[0].r_multiple.expect("a stop was set"), -1.0);
}

#[test]
fn commission_on_both_legs_is_taken_out_of_the_round_trip() {
    let ledger = [
        entry(1, 0, Side::Buy, 2.0, 100.0, 1.0),
        exit(2, 5, Side::Sell, 2.0, 105.0, 9.0, 1.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    // Ten points of gross, less one unit of commission at each end.
    approx(trips[0].realised, 8.0);
}

#[test]
fn a_partial_close_makes_one_round_trip_per_exit() {
    let ledger = [
        Trade {
            risk_per_unit: Some(1.0),
            ..entry(1, 0, Side::Buy, 4.0, 100.0, 0.0)
        },
        exit(2, 3, Side::Sell, 1.0, 105.0, 5.0, 0.0),
        exit(3, 7, Side::Sell, 3.0, 95.0, -15.0, 0.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    assert_eq!(trips.len(), 2);
    assert_eq!(trips[0].qty + trips[1].qty, 4.0);
    assert_eq!(trips[0].qty, 1.0);
    approx(trips[0].realised, 5.0);
    approx(trips[0].r_multiple.expect("a stop was set"), 5.0);
    assert_eq!(trips[1].qty, 3.0);
    approx(trips[1].realised, -15.0);
    // Three units risking one point each is three units of risk, not one.
    approx(trips[1].r_multiple.expect("a stop was set"), -5.0);
}

#[test]
fn scaling_in_pairs_each_lot_against_its_own_entry_price() {
    let ledger = [
        entry(1, 0, Side::Buy, 1.0, 100.0, 0.0),
        entry(2, 2, Side::Buy, 1.0, 102.0, 0.0),
        exit(3, 5, Side::Sell, 2.0, 104.0, 6.0, 0.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    assert_eq!(trips.len(), 2);
    assert_eq!(trips[0].entry, 100.0);
    approx(trips[0].realised, 4.0);
    assert_eq!(trips[1].entry, 102.0);
    approx(trips[1].realised, 2.0);
    assert_eq!(trips[0].opened, ts(0));
    assert_eq!(trips[1].opened, ts(2));
    // Whatever the split, the parts add up to what the exit actually realised.
    approx(trips[0].realised + trips[1].realised, 6.0);
}

#[test]
fn the_multiplier_scales_both_the_result_and_the_r_multiple() {
    let ledger = [
        Trade {
            risk_per_unit: Some(5.0),
            ..entry(1, 0, Side::Buy, 1.0, 17_000.0, 0.0)
        },
        exit(2, 5, Side::Sell, 1.0, 17_010.0, 200.0, 0.0),
    ];
    let trips = round_trips(&ledger, 20.0);
    approx(trips[0].realised, 200.0);
    // $100 of risk against $200 of profit, whatever the point value.
    approx(trips[0].r_multiple.expect("a stop was set"), 2.0);
}

#[test]
fn a_position_still_open_is_not_yet_a_round_trip() {
    let open_only = [entry(1, 0, Side::Buy, 2.0, 100.0, 0.0)];
    assert!(round_trips(&open_only, 1.0).is_empty());

    let half_closed = [
        entry(1, 0, Side::Buy, 2.0, 100.0, 0.0),
        exit(2, 5, Side::Sell, 1.0, 110.0, 10.0, 0.0),
    ];
    let trips = round_trips(&half_closed, 1.0);
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].qty, 1.0);
}

#[test]
fn a_trade_without_a_stop_has_no_r_multiple_and_is_left_out_of_the_average() {
    let ledger = [
        entry(1, 0, Side::Buy, 1.0, 100.0, 0.0),
        exit(2, 1, Side::Sell, 1.0, 110.0, 10.0, 0.0),
        Trade {
            risk_per_unit: Some(2.0),
            ..entry(3, 2, Side::Buy, 1.0, 100.0, 0.0)
        },
        exit(4, 3, Side::Sell, 1.0, 104.0, 4.0, 0.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    assert_eq!(trips[0].r_multiple, None);
    approx(trips[1].r_multiple.expect("a stop was set"), 2.0);

    let s = summary(1_000.0, &trips);
    // Only the trade that defined its risk counts; the other is absent, not zero.
    approx(s.average_r.expect("one R is defined"), 2.0);
}

#[test]
fn a_stop_on_the_entry_price_is_no_risk_at_all_rather_than_infinite_r() {
    let ledger = [
        Trade {
            risk_per_unit: Some(0.0),
            ..entry(1, 0, Side::Buy, 1.0, 100.0, 0.0)
        },
        exit(2, 1, Side::Sell, 1.0, 110.0, 10.0, 0.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    assert_eq!(trips[0].r_multiple, None);
    assert_eq!(summary(1_000.0, &trips).average_r, None);
}

#[test]
fn the_exits_assumption_is_carried_onto_the_round_trip_and_counted() {
    let ledger = [
        entry(1, 0, Side::Buy, 1.0, 100.0, 0.0),
        Trade {
            assumption: Assumption::SlFirst,
            ..exit(2, 1, Side::Sell, 1.0, 98.0, -2.0, 0.0)
        },
        entry(3, 2, Side::Buy, 1.0, 100.0, 0.0),
        Trade {
            assumption: Assumption::ResolvedByTicks,
            ..exit(4, 3, Side::Sell, 1.0, 104.0, 4.0, 0.0)
        },
        entry(5, 4, Side::Buy, 1.0, 100.0, 0.0),
        exit(6, 5, Side::Sell, 1.0, 101.0, 1.0, 0.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    assert_eq!(trips[0].assumption, Assumption::SlFirst);
    assert_eq!(trips[1].assumption, Assumption::ResolvedByTicks);
    assert_eq!(trips[2].assumption, Assumption::None);
    // Only the pessimistic guess is flagged; ticks settled the second one.
    assert_eq!(summary(1_000.0, &trips).flagged_trades, 1);
}

#[test]
fn a_partial_close_flags_only_the_exit_that_assumed() {
    let ledger = [
        entry(1, 0, Side::Buy, 2.0, 100.0, 0.0),
        Trade {
            assumption: Assumption::SlFirst,
            ..exit(2, 1, Side::Sell, 1.0, 98.0, -2.0, 0.0)
        },
        exit(3, 2, Side::Sell, 1.0, 103.0, 3.0, 0.0),
    ];
    let trips = round_trips(&ledger, 1.0);
    assert_eq!(summary(1_000.0, &trips).flagged_trades, 1);
}

#[test]
fn profit_factor_is_undefined_rather_than_infinite_when_nothing_was_lost() {
    let s = summary(1_000.0, &[trip(10.0, None, Assumption::None)]);
    assert_eq!(s.profit_factor, None);
    assert_eq!(s.losses, 0);
    approx(s.average_loss, 0.0);
    approx(s.gross_loss, 0.0);

    let none = summary(1_000.0, &[]);
    assert_eq!(none.profit_factor, None);
    assert_eq!(none.average_r, None);
    approx(none.win_rate, 0.0);
    approx(none.expectancy, 0.0);
    approx(none.max_drawdown, 0.0);
}

#[test]
fn the_summary_totals_the_round_trips() {
    let trips = [
        trip(100.0, Some(2.0), Assumption::None),
        trip(-50.0, Some(-1.0), Assumption::None),
        trip(25.0, None, Assumption::None),
    ];
    let s = summary(1_000.0, &trips);
    assert_eq!(s.trades, 3);
    assert_eq!(s.wins, 2);
    assert_eq!(s.losses, 1);
    approx(s.win_rate, 2.0 / 3.0);
    approx(s.net, 75.0);
    approx(s.gross_profit, 125.0);
    approx(s.gross_loss, 50.0);
    approx(s.profit_factor.expect("something was lost"), 2.5);
    approx(s.average_win, 62.5);
    approx(s.average_loss, 50.0);
    approx(s.expectancy, 25.0);
    approx(s.average_r.expect("two Rs are defined"), 0.5);
}

#[test]
fn max_drawdown_is_the_deepest_fall_from_a_peak() {
    // 1000 -> 1100 -> 800 -> 850 -> 750 -> 770. The peak is 1100 and the
    // trough after it is 750, so the worst run costs 350 even though no single
    // trade lost that much.
    let trips: Vec<RoundTrip> = [100.0, -300.0, 50.0, -100.0, 20.0]
        .iter()
        .map(|r| trip(*r, None, Assumption::None))
        .collect();
    let s = summary(1_000.0, &trips);
    approx(s.max_drawdown, 350.0);
    approx(s.net, -230.0);
}

#[test]
fn an_account_that_only_goes_up_has_no_drawdown() {
    let trips = [
        trip(10.0, None, Assumption::None),
        trip(5.0, None, Assumption::None),
    ];
    approx(summary(1_000.0, &trips).max_drawdown, 0.0);
}

#[test]
fn the_equity_curve_starts_at_the_first_fill_and_has_a_point_per_fill() {
    let ledger = [
        entry(1, 0, Side::Buy, 1.0, 100.0, 1.0),
        exit(2, 5, Side::Sell, 1.0, 110.0, 9.0, 1.0),
    ];
    let curve = equity_curve(1_000.0, &ledger);
    assert_eq!(curve.len(), 3);
    assert_eq!(curve[0].cursor, ts(0));
    approx(curve[0].balance, 1_000.0);
    // The entry costs its commission before the trade has gone anywhere.
    approx(curve[1].balance, 999.0);
    assert_eq!(curve[2].cursor, ts(5));
    approx(curve[2].balance, 1_008.0);

    assert!(equity_curve(1_000.0, &[]).is_empty());
}
