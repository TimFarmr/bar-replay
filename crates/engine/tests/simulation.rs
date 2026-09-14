//! The fill, determinism and rollback tests required by spec §6.
//!
//! Bars here are synthetic and hand-built. Real market data is used elsewhere
//! (the golden test); what these need is a bar whose shape is *exactly* the
//! awkward case under test, which real data rarely supplies on demand.

use replay_core::{Bar, Tick, Timestamp};
use replay_engine::order::{Assumption, Reason, Role};
use replay_engine::session::{Event, LoggedEvent};
use replay_engine::sim::{simulate, NoTicks, SimConfig, SimState, TickSource};
use replay_engine::{OrderKind, Side};

fn cfg() -> SimConfig {
    SimConfig {
        starting_balance: 10_000.0,
        spread: 0.0,
        commission_per_unit: 0.0,
        multiplier: 1.0,
    }
}

fn ts(minute: i64) -> Timestamp {
    Timestamp(minute * Timestamp::MINUTE)
}

fn bar(minute: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
    Bar {
        ts: ts(minute),
        open: o,
        high: h,
        low: l,
        close: c,
        volume: 1.0,
    }
}

fn at(seq: u64, minute: i64, event: Event) -> LoggedEvent {
    LoggedEvent {
        seq,
        cursor: ts(minute),
        event,
    }
}

fn market(side: Side, qty: f64, sl: Option<f64>, tp: Option<f64>) -> Event {
    Event::OrderPlace {
        side,
        kind: OrderKind::Market,
        qty,
        price: None,
        sl,
        tp,
    }
}

/// A flat series that a test can perturb where it matters.
fn flat(n: i64) -> Vec<Bar> {
    (0..n).map(|m| bar(m, 100.0, 100.5, 99.5, 100.0)).collect()
}

fn run(bars: &[Bar], events: &[LoggedEvent], cursor_minute: i64) -> SimState {
    simulate(cfg(), bars, events, ts(cursor_minute), &NoTicks)
}

#[test]
fn a_market_order_fills_at_the_close_of_the_bar_the_trader_is_looking_at() {
    let bars = flat(5);
    let st = run(&bars, &[at(1, 1, market(Side::Buy, 2.0, None, None))], 4);
    let pos = st.position.expect("a position should be open");
    assert_eq!(pos.side, Side::Buy);
    assert_eq!(pos.qty, 2.0);
    assert_eq!(pos.avg_entry, 100.0);
    assert_eq!(st.trades.len(), 1);
    assert_eq!(st.trades[0].role, Role::Entry);
    assert_eq!(st.trades[0].reason, Reason::Market);
}

/// Spec §6: "limit order not touched".
#[test]
fn a_limit_order_the_market_never_reaches_stays_working_and_trades_nothing() {
    let bars = flat(10);
    let events = [at(
        1,
        1,
        Event::OrderPlace {
            side: Side::Buy,
            kind: OrderKind::Limit,
            qty: 1.0,
            price: Some(90.0),
            sl: None,
            tp: None,
        },
    )];
    let st = run(&bars, &events, 9);
    assert!(st.trades.is_empty(), "nothing should have filled");
    assert!(st.position.is_none());
    assert_eq!(st.working.len(), 1, "the order is still resting");
    assert_eq!(st.balance, 10_000.0);
}

/// Spec §6: "gap-open through a stop".
#[test]
fn a_stop_jumped_by_a_gap_fills_at_the_open_not_at_the_stop_price() {
    let mut bars = flat(4);
    // Minute 3 gaps far below the stop at 99.
    bars.push(bar(4, 90.0, 91.0, 89.0, 90.5));
    let events = [at(1, 1, market(Side::Buy, 1.0, Some(99.0), None))];
    let st = run(&bars, &events, 5);

    let exit = st
        .trades
        .iter()
        .find(|t| t.role == Role::Exit)
        .expect("the stop must have fired");
    assert_eq!(exit.reason, Reason::Sl);
    assert_eq!(
        exit.price, 90.0,
        "the trader got the gapped open, not the stop price"
    );
    assert!(st.position.is_none());
    // Entered at 100, out at 90: a 10.0 loss on 1 unit.
    assert_eq!(st.balance, 9_990.0);
}

/// Spec §6: "SL and TP inside one bar", and §5.3: never silently pick the
/// favourable outcome.
#[test]
fn a_bar_hitting_both_stop_and_target_takes_the_stop_and_marks_the_trade() {
    let mut bars = flat(2);
    bars.push(bar(2, 100.0, 102.0, 98.0, 101.0));
    let events = [at(1, 1, market(Side::Buy, 1.0, Some(99.0), Some(101.0)))];
    let st = run(&bars, &events, 3);

    let exit = st.trades.iter().find(|t| t.role == Role::Exit).unwrap();
    assert_eq!(exit.reason, Reason::Sl);
    assert_eq!(exit.assumption, Assumption::SlFirst);
    assert_eq!(exit.price, 99.0);
}

/// Ticks, when the session has them, settle what OHLC cannot (ADR 0013).
#[test]
fn ticks_can_overturn_the_pessimistic_assumption() {
    struct Ticks;
    impl TickSource for Ticks {
        fn ticks_in(&self, minute: Timestamp) -> Option<Vec<Tick>> {
            (minute == ts(2)).then(|| {
                // Price reached the target before it ever reached the stop.
                vec![
                    Tick {
                        ts: minute,
                        bid: 101.5,
                        ask: 101.5,
                    },
                    Tick {
                        ts: minute,
                        bid: 98.0,
                        ask: 98.0,
                    },
                ]
            })
        }
    }
    let mut bars = flat(2);
    bars.push(bar(2, 100.0, 102.0, 98.0, 101.0));
    let events = [at(1, 1, market(Side::Buy, 1.0, Some(99.0), Some(101.0)))];
    let st = simulate(cfg(), &bars, &events, ts(3), &Ticks);

    let exit = st.trades.iter().find(|t| t.role == Role::Exit).unwrap();
    assert_eq!(exit.reason, Reason::Tp);
    assert_eq!(exit.assumption, Assumption::ResolvedByTicks);
    assert_eq!(st.balance, 10_001.0);
}

/// Spec §6: "partial close".
#[test]
fn closing_part_of_a_position_leaves_the_rest_open() {
    let mut bars = flat(3);
    bars.push(bar(3, 105.0, 105.5, 104.5, 105.0));
    let events = [
        at(1, 1, market(Side::Buy, 4.0, None, None)),
        at(2, 3, Event::PositionClose { qty: Some(1.0) }),
    ];
    let st = run(&bars, &events, 4);

    let pos = st.position.expect("three units should remain");
    assert_eq!(pos.qty, 3.0);
    assert_eq!(pos.avg_entry, 100.0);

    let exits: Vec<_> = st.trades.iter().filter(|t| t.role == Role::Exit).collect();
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].qty, 1.0);
    assert_eq!(exits[0].reason, Reason::Close);
    assert_eq!(st.balance, 10_005.0, "one unit banked five points");
}

/// Spec §6: "break-even move".
#[test]
fn moving_the_stop_to_break_even_makes_a_retrace_cost_nothing() {
    let mut bars = flat(2);
    bars.push(bar(2, 103.0, 104.0, 102.5, 103.5)); // runs up
    bars.push(bar(3, 103.0, 103.5, 99.0, 99.5)); // falls back through entry

    let events = [
        at(1, 1, market(Side::Buy, 2.0, Some(95.0), None)),
        // At minute 2 the trader pulls the stop up to the entry price.
        at(
            2,
            2,
            Event::OrderModify {
                order: None,
                sl: Some(100.0),
                tp: None,
            },
        ),
    ];
    let st = run(&bars, &events, 4);

    let exit = st.trades.iter().find(|t| t.role == Role::Exit).unwrap();
    assert_eq!(exit.reason, Reason::Sl);
    assert_eq!(exit.price, 100.0);
    assert_eq!(st.balance, 10_000.0, "break-even means exactly no change");
    assert!(st.position.is_none());
}

/// Spec §6: "run the same scripted session twice, diff the trade logs".
#[test]
fn the_same_scripted_session_twice_produces_byte_identical_ledgers() {
    let mut bars = flat(6);
    bars.push(bar(6, 101.0, 103.0, 100.5, 102.5));
    bars.push(bar(7, 102.0, 102.5, 97.0, 98.0));
    let events = [
        at(1, 1, market(Side::Buy, 3.0, Some(98.5), Some(103.0))),
        at(2, 2, Event::PositionClose { qty: Some(1.0) }),
        at(
            3,
            3,
            Event::OrderModify {
                order: None,
                sl: Some(99.0),
                tp: Some(102.0),
            },
        ),
    ];

    let first = render(&run(&bars, &events, 8));
    let second = render(&run(&bars, &events, 8));
    assert_eq!(first, second, "the ledger must be reproducible");
    assert!(first.contains("exit"), "the run should actually trade");
}

/// Spec §6: "place and fill an order, step the cursor back past the fill time,
/// assert the order/position state has fully reverted" (invariant §5.5).
#[test]
fn stepping_back_past_a_fill_leaves_no_ghost_of_it() {
    let bars = flat(10);
    let events = [at(1, 5, market(Side::Buy, 2.0, None, None))];

    let after = run(&bars, &events, 7);
    assert!(after.position.is_some());
    assert_eq!(after.trades.len(), 1);

    // Rewind to before the order was ever placed.
    let before = run(&bars, &events, 4);
    assert!(
        before.position.is_none(),
        "the position must not survive the rewind"
    );
    assert!(before.trades.is_empty(), "the ledger must be empty again");
    assert!(before.working.is_empty());
    assert_eq!(before.balance, 10_000.0);

    // And stepping forward again reproduces exactly the earlier state.
    assert_eq!(render(&run(&bars, &events, 7)), render(&after));
}

#[test]
fn commission_and_spread_both_come_out_of_the_balance() {
    let charged = SimConfig {
        spread: 0.10,
        commission_per_unit: 0.50,
        ..cfg()
    };
    let mut bars = flat(2);
    bars.push(bar(2, 100.0, 100.5, 99.5, 100.0));
    let events = [
        at(1, 1, market(Side::Buy, 1.0, None, None)),
        at(2, 2, Event::PositionClose { qty: None }),
    ];
    let st = simulate(charged, &bars, &events, ts(3), &NoTicks);

    // Bought the ask at 100.10, sold the bid at 100.00: ten cents of spread,
    // plus fifty cents of commission on each leg.
    assert_eq!(st.trades.len(), 2);
    assert_eq!(st.trades[0].price, 100.10);
    assert_eq!(st.trades[1].price, 100.00);
    assert!((st.balance - 9_998.90).abs() < 1e-9, "{}", st.balance);
}

/// Fixed precision, so "byte-identical" is well defined (ADR 0010).
fn render(st: &SimState) -> String {
    let mut out = format!("balance {:.2}\n", st.balance);
    for t in &st.trades {
        out.push_str(&format!(
            "{} {} {:?} {:?} {:?} qty {:.4} @ {:.5} comm {:.2} pnl {:.2} {:?}\n",
            t.seq,
            t.cursor.0,
            t.side,
            t.role,
            t.reason,
            t.qty,
            t.price,
            t.commission,
            t.realised,
            t.assumption,
        ));
    }
    out.to_lowercase()
}

/// Adding to a position with a bare market order must not strip the stop that
/// is already protecting it. Silently removing a stop is the single most
/// dangerous thing a simulator could get wrong.
#[test]
fn adding_to_a_position_never_clears_the_stop_already_set() {
    let mut bars = flat(3);
    bars.push(bar(3, 100.0, 100.5, 99.5, 100.0));
    bars.push(bar(4, 100.0, 100.5, 94.0, 95.0)); // dips through the stop
    let events = [
        at(1, 1, market(Side::Buy, 1.0, Some(98.0), None)),
        // A second buy, with no protection named.
        at(2, 2, market(Side::Buy, 1.0, None, None)),
    ];
    let st = run(&bars, &events, 5);

    let exit = st
        .trades
        .iter()
        .find(|t| t.role == Role::Exit)
        .expect("the original stop must still have been guarding the position");
    assert_eq!(exit.reason, Reason::Sl);
    assert_eq!(exit.price, 98.0);
    assert_eq!(exit.qty, 2.0, "both units were protected");
}

/// Naming protection on a later order does replace it — that is an explicit
/// instruction, not an accident.
#[test]
fn a_market_order_that_names_a_stop_does_replace_the_old_one() {
    let mut bars = flat(3);
    bars.push(bar(3, 100.0, 100.5, 96.5, 97.0));
    let events = [
        at(1, 1, market(Side::Buy, 1.0, Some(90.0), None)),
        at(2, 2, market(Side::Buy, 1.0, Some(97.5), None)),
    ];
    let st = run(&bars, &events, 4);
    let exit = st.trades.iter().find(|t| t.role == Role::Exit).unwrap();
    assert_eq!(exit.price, 97.5, "the newer stop applies");
}
