//! Stage A4's close condition: a **full trading day in process** — no server,
//! no database, no clock, no sleeping — plus the replay guarantee the whole
//! event-log design rests on.
//!
//! An integration test is its own crate, so the lib's `cfg_attr(test, ...)`
//! allowance does not reach it and the workspace `unwrap_used` deny has to be
//! lifted here explicitly. A panic in a test is a failed test, which is the
//! point of one.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use broker::{FeeSchedule, FillPolicy, Limits, MockBroker};
use domain::{ClientOrderId, Money, ParticipantId, Px, Qty, Side, Symbol, Timestamp};
use engine::{Command, Engine, EngineError, Journaled};

fn who(name: &str) -> ParticipantId {
    ParticipantId::parse(name).unwrap()
}
fn sym(s: &str) -> Symbol {
    Symbol::parse(s).unwrap()
}
fn px(s: &str) -> Px {
    Px::parse(s).unwrap()
}
fn money(s: &str) -> Money {
    Money::parse(s).unwrap()
}
fn qty(n: i64) -> Qty {
    Qty::new(n).unwrap()
}
fn oid(n: u64) -> ClientOrderId {
    ClientOrderId::new(n)
}

/// Every observable fact about the engine, rendered deterministically. Two
/// engines agreeing on this agree on everything a participant can see.
fn snapshot<B: broker::Broker>(e: &Engine<B>) -> Vec<String> {
    let mut out = vec![format!("seq={}", e.seq())];
    for p in e.participants() {
        out.push(format!(
            "participant {} cash={} realized={} start={}",
            p.participant(),
            p.cash(),
            p.realized_pnl(),
            p.starting_cash()
        ));
        for pos in p.positions() {
            out.push(format!(
                "  position {} qty={} cost={} avg={:?}",
                pos.symbol(),
                pos.qty(),
                pos.cost(),
                pos.avg_cost().map(|a| a.to_string())
            ));
        }
    }
    for o in e.orders() {
        out.push(format!(
            "order {} {} {:?} qty={} px={} state={} filled={} cost={} fees={} replaces={:?}",
            o.id,
            o.symbol,
            o.side,
            o.qty,
            o.limit_px,
            o.state.name(),
            o.state.filled(),
            o.state.cost(),
            e.fee_of(o.id),
            o.replaces.map(|r| r.get())
        ));
    }
    out
}

/// Drives the scenario, returning the engine and the full log.
fn run_day() -> (Engine<MockBroker>, Vec<Journaled>) {
    let mut e = Engine::new(MockBroker::simple(7));
    let mut log = Vec::new();
    let mut t = 0;
    let mut go = |e: &mut Engine<MockBroker>, log: &mut Vec<Journaled>, cmd: Command| {
        t += 1_000;
        log.extend(e.execute(Timestamp::from_millis(t), cmd).unwrap());
    };

    for name in ["alice", "bob"] {
        go(
            &mut e,
            &mut log,
            Command::CreateParticipant {
                participant: who(name),
                starting_cash: money("100000"),
            },
        );
    }

    // alice: buy 100 AAPL @ 10, take 40, cancel the rest.
    go(
        &mut e,
        &mut log,
        Command::SubmitOrder {
            id: oid(1),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(100),
            limit_px: px("10"),
        },
    );
    go(
        &mut e,
        &mut log,
        Command::Execute {
            id: oid(1),
            qty: qty(40),
            px: px("10"),
        },
    );
    go(&mut e, &mut log, Command::CancelOrder { id: oid(1) });

    // alice: buy 50 MSFT @ 20, filled by the broker's own policy.
    go(
        &mut e,
        &mut log,
        Command::SubmitOrder {
            id: oid(2),
            participant: who("alice"),
            symbol: sym("MSFT"),
            side: Side::Buy,
            qty: qty(50),
            limit_px: px("20"),
        },
    );
    go(&mut e, &mut log, Command::AutoExecute { id: oid(2) });

    // bob: submit 200 AAPL @ 12, replace down to 100 @ 11, then fill.
    go(
        &mut e,
        &mut log,
        Command::SubmitOrder {
            id: oid(3),
            participant: who("bob"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(200),
            limit_px: px("12"),
        },
    );
    go(
        &mut e,
        &mut log,
        Command::ReplaceOrder {
            id: oid(3),
            replacement_id: oid(4),
            qty: qty(100),
            limit_px: px("11"),
        },
    );
    go(&mut e, &mut log, Command::AutoExecute { id: oid(4) });

    // alice sells 20 of the AAPL she holds.
    go(
        &mut e,
        &mut log,
        Command::SubmitOrder {
            id: oid(5),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Sell,
            qty: qty(20),
            limit_px: px("11"),
        },
    );
    go(&mut e, &mut log, Command::AutoExecute { id: oid(5) });

    for (s, p) in [("AAPL", "11"), ("MSFT", "21")] {
        go(
            &mut e,
            &mut log,
            Command::UpdateMark {
                symbol: sym(s),
                px: px(p),
            },
        );
    }

    (e, log)
}

#[test]
fn a_full_trading_day_produces_hand_computed_portfolios() {
    let (e, _) = run_day();

    // --- alice ---------------------------------------------------------
    // buy  40 AAPL @ 10 = 400      cash 100000 - 400  = 99600
    // buy  50 MSFT @ 20 = 1000     cash  99600 - 1000 = 98600
    // sell 20 AAPL @ 11 = 220, basis out 400 x 20/40 = 200 -> realized 20
    //                              cash  98600 + 220  = 98820
    let alice = e.portfolio(&who("alice")).unwrap();
    assert_eq!(alice.cash(), money("98820"));
    assert_eq!(alice.realized_pnl(), money("20"));

    let aapl = alice.position(&sym("AAPL")).unwrap();
    assert_eq!(aapl.qty(), qty(20));
    assert_eq!(aapl.cost(), money("200"));

    let msft = alice.position(&sym("MSFT")).unwrap();
    assert_eq!(msft.qty(), qty(50));
    assert_eq!(msft.cost(), money("1000"));

    // marks AAPL 11, MSFT 21: unrealized 20 + 50 = 70
    let marks = e.marks();
    assert_eq!(alice.unrealized_pnl(marks).unwrap(), money("70"));
    assert_eq!(alice.total_value(marks).unwrap(), money("100090"));
    assert_eq!(
        alice.total_value(marks).unwrap(),
        money("100000")
            .checked_add(alice.realized_pnl())
            .unwrap()
            .checked_add(alice.unrealized_pnl(marks).unwrap())
            .unwrap()
    );

    // --- bob -----------------------------------------------------------
    // replaced 200 @ 12 down to 100 @ 11, filled -> 1100 out, no P&L at mark 11
    let bob = e.portfolio(&who("bob")).unwrap();
    assert_eq!(bob.cash(), money("98900"));
    assert_eq!(bob.realized_pnl(), Money::ZERO);
    assert_eq!(bob.position(&sym("AAPL")).unwrap().qty(), qty(100));
    assert_eq!(bob.unrealized_pnl(marks).unwrap(), Money::ZERO);
    assert_eq!(bob.total_value(marks).unwrap(), money("100000"));

    // --- order states ---------------------------------------------------
    assert_eq!(e.order(oid(1)).unwrap().state.name(), "CANCELLED");
    assert_eq!(e.order(oid(1)).unwrap().state.filled(), qty(40));
    assert_eq!(e.order(oid(2)).unwrap().state.name(), "FILLED");
    assert_eq!(e.order(oid(3)).unwrap().state.name(), "CANCELLED");
    assert_eq!(e.order(oid(4)).unwrap().replaces, Some(oid(3)));
    assert_eq!(e.order(oid(4)).unwrap().state.name(), "FILLED");

    // Nothing is left working at the end of the day.
    assert_eq!(e.working_orders_of(&who("alice")).count(), 0);
    assert_eq!(e.working_orders_of(&who("bob")).count(), 0);
}

#[test]
fn replaying_the_log_reproduces_the_state_exactly() {
    let (live, log) = run_day();

    // A **different seed**: if replay consulted the broker at all, this engine
    // would diverge. It does not, because every broker decision is already a
    // recorded fact.
    let replayed = Engine::replay(MockBroker::simple(999), log.clone()).unwrap();

    assert_eq!(snapshot(&replayed), snapshot(&live));
    assert_eq!(replayed.seq(), live.seq());
    assert_eq!(replayed.marks(), live.marks());
}

#[test]
fn the_log_is_a_contiguous_total_order() {
    let (_, log) = run_day();
    let seqs: Vec<u64> = log.iter().map(|j| j.seq).collect();
    assert_eq!(seqs, (1..=log.len() as u64).collect::<Vec<_>>());
    assert!(log.windows(2).all(|w| w[0].at <= w[1].at));
}

#[test]
fn a_refused_command_produces_no_events_and_no_change() {
    let (mut e, _) = run_day();
    let before = snapshot(&e);
    let at = Timestamp::from_millis(999_999);

    // Each of these is refused for a different reason; none may leave a trace.
    let refused = [
        Command::CreateParticipant {
            participant: who("alice"),
            starting_cash: money("1"),
        },
        Command::SubmitOrder {
            id: oid(1),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(1),
            limit_px: px("10"),
        },
        Command::CancelOrder { id: oid(1) },
        Command::Execute {
            id: oid(2),
            qty: qty(1),
            px: px("20"),
        },
        Command::SubmitOrder {
            id: oid(90),
            participant: who("carol"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(1),
            limit_px: px("10"),
        },
    ];

    for command in refused {
        let err = e.execute(at, command.clone()).unwrap_err();
        assert_eq!(snapshot(&e), before, "{command:?} left a trace: {err}");
    }
}

#[test]
fn working_orders_reserve_cash_so_two_orders_cannot_spend_it_twice() {
    let mut e = Engine::new(MockBroker::simple(0));
    let at = Timestamp::from_millis(0);
    e.execute(
        at,
        Command::CreateParticipant {
            participant: who("alice"),
            starting_cash: money("1000"),
        },
    )
    .unwrap();

    let buy = |id: u64, n: i64| Command::SubmitOrder {
        id: oid(id),
        participant: who("alice"),
        symbol: sym("AAPL"),
        side: Side::Buy,
        qty: qty(n),
        limit_px: px("10"),
    };

    // 60 shares @ 10 = 600, working and unfilled.
    e.execute(at, buy(1, 60)).unwrap();

    // A second order for 600 would fit the *balance* but not the *free* cash.
    // Without reservations both would pass here and the second fill would then
    // fail deep in the book, which is far too late.
    assert!(matches!(
        e.execute(at, buy(2, 60)),
        Err(EngineError::InsufficientAvailableCash { .. })
    ));

    // 40 more shares = 400, exactly the remainder. Allowed.
    e.execute(at, buy(3, 40)).unwrap();

    // Cancelling the first releases its reservation.
    e.execute(at, Command::CancelOrder { id: oid(1) }).unwrap();
    e.execute(at, buy(4, 60)).unwrap();
}

#[test]
fn a_replace_releases_the_reservation_it_is_about_to_cancel() {
    let mut e = Engine::new(MockBroker::simple(0));
    let at = Timestamp::from_millis(0);
    e.execute(
        at,
        Command::CreateParticipant {
            participant: who("alice"),
            starting_cash: money("1000"),
        },
    )
    .unwrap();
    e.execute(
        at,
        Command::SubmitOrder {
            id: oid(1),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(100),
            limit_px: px("10"),
        },
    )
    .unwrap();

    // The whole balance is committed. Replacing at the same size must still
    // work: the command that takes the new reservation releases the old one.
    e.execute(
        at,
        Command::ReplaceOrder {
            id: oid(1),
            replacement_id: oid(2),
            qty: qty(100),
            limit_px: px("10"),
        },
    )
    .unwrap();

    assert_eq!(e.order(oid(1)).unwrap().state.name(), "CANCELLED");
    assert_eq!(e.order(oid(2)).unwrap().state.name(), "ACKNOWLEDGED");
}

#[test]
fn positions_are_reserved_against_working_sell_orders() {
    let mut e = Engine::new(MockBroker::simple(0));
    let at = Timestamp::from_millis(0);
    e.execute(
        at,
        Command::CreateParticipant {
            participant: who("alice"),
            starting_cash: money("10000"),
        },
    )
    .unwrap();
    e.execute(
        at,
        Command::SubmitOrder {
            id: oid(1),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(100),
            limit_px: px("10"),
        },
    )
    .unwrap();
    e.execute(at, Command::AutoExecute { id: oid(1) }).unwrap();

    let sell = |id: u64, n: i64| Command::SubmitOrder {
        id: oid(id),
        participant: who("alice"),
        symbol: sym("AAPL"),
        side: Side::Sell,
        qty: qty(n),
        limit_px: px("11"),
    };

    e.execute(at, sell(2, 80)).unwrap();
    // 100 held, 80 already committed: 30 more would be a short.
    assert!(matches!(
        e.execute(at, sell(3, 30)),
        Err(EngineError::InsufficientAvailablePosition { .. })
    ));
    e.execute(at, sell(4, 20)).unwrap();
}

#[test]
fn partial_fills_from_the_broker_reconcile_to_the_ordered_quantity() {
    let mut e = Engine::new(MockBroker::new(
        11,
        FillPolicy::Partial { max_slices: 4 },
        FeeSchedule { bps: 10 },
        Limits::default(),
    ));
    let at = Timestamp::from_millis(0);
    e.execute(
        at,
        Command::CreateParticipant {
            participant: who("alice"),
            starting_cash: money("100000"),
        },
    )
    .unwrap();
    e.execute(
        at,
        Command::SubmitOrder {
            id: oid(1),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(997),
            limit_px: px("10"),
        },
    )
    .unwrap();

    let mut fills = 0;
    while !e.order(oid(1)).unwrap().state.is_terminal() {
        e.execute(at, Command::AutoExecute { id: oid(1) }).unwrap();
        fills += 1;
        assert!(fills < 10, "the fill policy failed to terminate");
    }

    assert!(fills > 1, "expected the order to fill in pieces");
    let order = e.order(oid(1)).unwrap();
    assert_eq!(order.state.name(), "FILLED");
    assert_eq!(order.state.filled(), qty(997));

    // 997 x 10 = 9970 notional, 10bp = 9.97 of fees capitalised into the basis.
    let alice = e.portfolio(&who("alice")).unwrap();
    assert_eq!(alice.position(&sym("AAPL")).unwrap().qty(), qty(997));
    assert_eq!(
        alice.position(&sym("AAPL")).unwrap().cost(),
        money("9979.97")
    );
    assert_eq!(alice.cash(), money("90020.03"));
    assert_eq!(alice.fees_paid(), money("9.97"));
    assert_eq!(e.fee_of(oid(1)), money("9.97"));
}
