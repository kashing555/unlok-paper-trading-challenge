//! Stage B1's close condition: projections rebuilt from the log equal the live
//! projections field for field, and a redelivered event is a proven no-op.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use broker::MockBroker;
use domain::{
    BrokerOrderId, ClientOrderId, Money, NewOrder, Order, OrderState, ParticipantId, Px, Qty,
    RejectReason, Side, Symbol, Timestamp,
};
use engine::{Command, Engine, Event, Journaled};
use store::{EventLog, InMemoryLog, SqliteLog};

fn who(n: &str) -> ParticipantId {
    ParticipantId::parse(n).unwrap()
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

fn an_order(state: OrderState) -> Box<Order> {
    let mut o = Order::submit(NewOrder {
        id: oid(1),
        participant: who("alice"),
        symbol: sym("AAPL"),
        side: Side::Buy,
        qty: qty(100),
        limit_px: px("10.0050"),
        at: Timestamp::from_millis(5),
    })
    .unwrap();
    o.state = state;
    o.replaces = Some(oid(99));
    Box::new(o)
}

/// One of **every** variant, so a new event type added without a mapping fails
/// here rather than at 3am against a log that will not load.
fn every_event() -> Vec<Event> {
    vec![
        Event::ParticipantCreated {
            participant: who("alice"),
            starting_cash: money("100000"),
        },
        Event::OrderSubmitted {
            order: an_order(OrderState::New),
        },
        Event::OrderAcknowledged {
            id: oid(1),
            broker_id: BrokerOrderId::new(7),
        },
        Event::OrderRejected {
            id: oid(2),
            reason: RejectReason::InsufficientCash,
        },
        Event::OrderFilled {
            id: oid(1),
            qty: qty(40),
            px: px("10.0050"),
            fee: money("0.4002"),
        },
        Event::OrderCancelled { id: oid(1) },
        Event::OrderReplaced {
            original: oid(1),
            replacement: an_order(OrderState::PartiallyFilled {
                broker_id: BrokerOrderId::new(8),
                filled: qty(40),
                cost: money("400.2000"),
            }),
        },
        Event::MarkUpdated {
            symbol: sym("MSFT"),
            px: px("20.1234"),
        },
        Event::DayClosed {
            day: domain::TradingDay::parse("2026-08-29").unwrap(),
            entries: vec![scoring::DayInput {
                participant: who("alice"),
                closing_value: money("100230"),
                prior_closing_value: money("100000"),
                turnover: money("2455.5"),
                active: true,
            }],
        },
    ]
}

fn journal(events: Vec<Event>) -> Vec<Journaled> {
    events
        .into_iter()
        .enumerate()
        .map(|(i, event)| Journaled {
            seq: i as u64 + 1,
            at: Timestamp::from_millis(1_000 + i as i64),
            event,
        })
        .collect()
}

#[test]
fn every_event_variant_survives_a_round_trip_unchanged() {
    let entries = journal(every_event());
    let mut log = SqliteLog::in_memory().unwrap();
    log.append(&entries).unwrap();

    // Exact equality, not "looks similar": sub-cent prices, capitalised fees
    // and a replaces link all have to come back bit for bit.
    assert_eq!(log.read_all().unwrap(), entries);
}

#[test]
fn re_appending_the_same_batch_is_a_no_op() {
    // Networks retry and processes crash mid-write. A duplicated fill is a real
    // position and real money, so `seq` is the primary key and a conflicting
    // insert does nothing.
    let entries = journal(every_event());
    for log in [
        &mut SqliteLog::in_memory().unwrap() as &mut dyn EventLog,
        &mut InMemoryLog::new(),
    ] {
        log.append(&entries).unwrap();
        log.append(&entries).unwrap();
        log.append(&entries[..3]).unwrap();
        assert_eq!(log.read_all().unwrap(), entries);
        assert_eq!(log.last_seq().unwrap(), entries.len() as u64);
    }
}

#[test]
fn the_log_survives_the_process_that_wrote_it() {
    let path = std::env::temp_dir().join(format!("ptc-store-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let entries = journal(every_event());

    {
        let mut log = SqliteLog::open(&path).unwrap();
        log.append(&entries).unwrap();
    } // connection dropped

    let reopened = SqliteLog::open(&path).unwrap();
    assert_eq!(reopened.read_all().unwrap(), entries);
    std::fs::remove_file(&path).unwrap();
}

/// The guarantee the whole design rests on, now end to end through SQLite.
#[test]
fn state_rebuilt_from_the_stored_log_matches_the_live_engine() {
    let mut live = Engine::new(MockBroker::simple(3));
    let mut log = SqliteLog::in_memory().unwrap();
    let mut t = 0;

    let mut run = |e: &mut Engine<MockBroker>, l: &mut SqliteLog, cmd: Command| {
        t += 1_000;
        let produced = e.execute(Timestamp::from_millis(t), cmd).unwrap();
        l.append(&produced).unwrap();
    };

    run(
        &mut live,
        &mut log,
        Command::CreateParticipant {
            participant: who("alice"),
            starting_cash: money("100000"),
        },
    );
    run(
        &mut live,
        &mut log,
        Command::SubmitOrder {
            id: oid(1),
            participant: who("alice"),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: qty(100),
            limit_px: px("10.0050"),
        },
    );
    run(
        &mut live,
        &mut log,
        Command::Execute {
            id: oid(1),
            qty: qty(40),
            px: px("10.0050"),
        },
    );
    run(&mut live, &mut log, Command::CancelOrder { id: oid(1) });
    run(
        &mut live,
        &mut log,
        Command::UpdateMark {
            symbol: sym("AAPL"),
            px: px("11.25"),
        },
    );

    // Different seed again: replay must not consult the broker.
    let rebuilt = Engine::replay(MockBroker::simple(4444), log.read_all().unwrap()).unwrap();

    let a = live.portfolio(&who("alice")).unwrap();
    let b = rebuilt.portfolio(&who("alice")).unwrap();
    assert_eq!(a.cash(), b.cash());
    assert_eq!(a.realized_pnl(), b.realized_pnl());
    assert_eq!(
        a.total_value(live.marks()).unwrap(),
        b.total_value(rebuilt.marks()).unwrap()
    );
    assert_eq!(a.position(&sym("AAPL")), b.position(&sym("AAPL")));
    assert_eq!(live.order(oid(1)).unwrap(), rebuilt.order(oid(1)).unwrap());
    assert_eq!(live.seq(), rebuilt.seq());
    assert_eq!(live.marks(), rebuilt.marks());

    // and the sub-cent arithmetic really did survive the JSON boundary
    assert_eq!(b.position(&sym("AAPL")).unwrap().cost(), money("400.2000"));
}
