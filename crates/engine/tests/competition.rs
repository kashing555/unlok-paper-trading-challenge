//! Closing days, publishing leaderboards, and the overall ladder — the brief's
//! "daily competition results" end to end over the engine.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use broker::MockBroker;
use domain::{ClientOrderId, Money, ParticipantId, Px, Qty, Side, Symbol, Timestamp, TradingDay};
use engine::{Command, Engine, EngineError};
use rust_decimal::Decimal;

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
fn day(s: &str) -> TradingDay {
    TradingDay::parse(s).unwrap()
}
fn dec(s: &str) -> Decimal {
    s.parse().unwrap()
}

struct Harness {
    engine: Engine<MockBroker>,
    t: i64,
}

impl Harness {
    fn new() -> Self {
        let mut h = Self {
            engine: Engine::new(MockBroker::simple(1)),
            t: 0,
        };
        for name in ["alice", "bob"] {
            h.run(Command::CreateParticipant {
                participant: who(name),
                starting_cash: money("100000"),
            });
        }
        h
    }

    fn run(&mut self, cmd: Command) {
        self.t += 1_000;
        self.engine
            .execute(Timestamp::from_millis(self.t), cmd)
            .unwrap();
    }

    fn try_run(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.t += 1_000;
        self.engine
            .execute(Timestamp::from_millis(self.t), cmd)
            .map(|_| ())
    }

    /// Buy `n` of AAPL at 10 and let the broker fill it completely.
    fn buy(&mut self, name: &str, id: u64, n: i64) {
        self.run(Command::SubmitOrder {
            id: ClientOrderId::new(id),
            participant: who(name),
            symbol: sym("AAPL"),
            side: Side::Buy,
            qty: Qty::new(n).unwrap(),
            limit_px: px("10"),
        });
        self.run(Command::AutoExecute {
            id: ClientOrderId::new(id),
        });
    }

    fn mark(&mut self, p: &str) {
        self.run(Command::UpdateMark {
            symbol: sym("AAPL"),
            px: px(p),
        });
    }
}

/// Two days of a two-participant competition, with every figure hand-computed.
fn two_days() -> Harness {
    let mut h = Harness::new();

    // Day 1: alice commits 1000, bob 500; the mark then rises to 12.
    h.buy("alice", 1, 100);
    h.buy("bob", 2, 50);
    h.mark("12");
    h.run(Command::CloseDay {
        day: day("2026-08-28"),
    });

    // Day 2: nobody trades, the mark falls back to 11.
    h.mark("11");
    h.run(Command::CloseDay {
        day: day("2026-08-29"),
    });
    h
}

#[test]
fn a_closed_day_publishes_hand_computed_results() {
    let h = two_days();
    let d1 = h.engine.leaderboard(day("2026-08-28")).unwrap();

    // alice: cash 99000 + 100 x 12 = 100200 -> +0.20%
    // bob:   cash 99500 +  50 x 12 = 100100 -> +0.10%
    assert_eq!(d1.rows[0].result.participant, who("alice"));
    assert_eq!(d1.rows[0].result.closing_value, money("100200"));
    assert_eq!(d1.rows[0].result.daily_pnl, money("200"));
    assert_eq!(d1.rows[0].result.daily_return, dec("0.002"));
    assert_eq!(d1.rows[0].result.turnover, money("1000"));

    assert_eq!(d1.rows[1].result.participant, who("bob"));
    assert_eq!(d1.rows[1].result.closing_value, money("100100"));
    assert_eq!(d1.rows[1].result.turnover, money("500"));
    assert!(d1.rows.iter().all(|r| r.result.active));
}

#[test]
fn the_second_day_measures_from_the_first_days_close() {
    let h = two_days();
    let d2 = h.engine.leaderboard(day("2026-08-29")).unwrap();

    // alice: 100100 against a 100200 baseline -> -100/100200
    // bob:   100050 against a 100100 baseline ->  -50/100100
    // bob loses less in percentage terms, so bob takes the day.
    assert_eq!(d2.rows[0].result.participant, who("bob"));
    assert_eq!(d2.rows[0].result.daily_return, dec("-0.0004995005"));
    assert_eq!(d2.rows[1].result.daily_return, dec("-0.0009980040"));

    // Nobody traded, but both still hold — held exposure is participation.
    assert!(d2.rows.iter().all(|r| r.result.turnover == Money::ZERO));
    assert!(d2.rows.iter().all(|r| r.result.active));
}

#[test]
fn the_ladder_compounds_the_two_days() {
    let l = two_days().engine.ladder().unwrap();

    // alice 100000 -> 100100 = +0.10%; bob 100000 -> 100050 = +0.05%
    assert_eq!(l[0].participant, who("alice"));
    assert_eq!(l[0].cumulative_return, dec("0.0010000000"));
    assert_eq!(l[0].rank, Some(1));
    assert_eq!(l[0].daily_wins, 1);

    assert_eq!(l[1].participant, who("bob"));
    assert_eq!(l[1].cumulative_return, dec("0.0005000000"));
    assert_eq!(l[1].daily_wins, 1);
    assert!(l.iter().all(|r| r.eligible && r.active_days == 2));
}

#[test]
fn closing_a_day_twice_changes_nothing() {
    let mut h = two_days();
    let before = h.engine.leaderboard(day("2026-08-29")).unwrap();
    let seq_before = h.engine.seq();

    // Even after the book moves, the published day does not.
    h.mark("50");
    h.run(Command::CloseDay {
        day: day("2026-08-29"),
    });

    assert_eq!(h.engine.leaderboard(day("2026-08-29")).unwrap(), before);
    assert_eq!(
        h.engine.seq(),
        seq_before + 1,
        "only the mark update should have been journalled"
    );
}

#[test]
fn a_day_cannot_be_closed_while_a_held_symbol_has_no_mark() {
    // Fail closed: a wrong closing value corrupts a board that is then
    // immutable, so refusing to close is the cheaper failure.
    let mut h = Harness::new();
    h.buy("alice", 1, 10);

    assert!(matches!(
        h.try_run(Command::CloseDay {
            day: day("2026-08-28")
        }),
        Err(EngineError::Portfolio(
            domain::PortfolioError::MissingMark { .. }
        ))
    ));
    assert!(h.engine.leaderboard(day("2026-08-28")).is_err());
}

#[test]
fn an_unclosed_day_has_no_leaderboard() {
    let h = Harness::new();
    assert_eq!(
        h.engine.leaderboard(day("2026-08-28")).unwrap_err(),
        EngineError::DayNotClosed(day("2026-08-28"))
    );
    assert!(h.engine.ladder().unwrap().is_empty());
}

#[test]
fn turnover_resets_at_the_close_and_accrues_on_both_sides() {
    let mut h = Harness::new();
    h.buy("alice", 1, 100); // 1000 of notional
    h.mark("12");
    h.run(Command::CloseDay {
        day: day("2026-08-28"),
    });

    // Day 2: sell 50 @ 12 = 600 of turnover. Gross, not net.
    h.run(Command::SubmitOrder {
        id: ClientOrderId::new(2),
        participant: who("alice"),
        symbol: sym("AAPL"),
        side: Side::Sell,
        qty: Qty::new(50).unwrap(),
        limit_px: px("12"),
    });
    h.run(Command::AutoExecute {
        id: ClientOrderId::new(2),
    });
    h.run(Command::CloseDay {
        day: day("2026-08-29"),
    });

    let d1 = h.engine.leaderboard(day("2026-08-28")).unwrap();
    let d2 = h.engine.leaderboard(day("2026-08-29")).unwrap();
    let alice_on = |b: &scoring::Leaderboard| {
        b.rows
            .iter()
            .find(|r| r.result.participant == who("alice"))
            .unwrap()
            .result
            .clone()
    };

    assert_eq!(alice_on(&d1).turnover, money("1000"));
    assert_eq!(alice_on(&d2).turnover, money("600"));

    // Day 2 is also the turnover tiebreak doing its job. alice sold at the
    // mark, so her return is 0.00% — exactly bob's, who did nothing. They tie
    // on the primary key, and **bob takes the day on lower turnover**: the same
    // result reached with less trading is the better result (ranking.md §2).
    assert_eq!(alice_on(&d2).daily_return, Decimal::ZERO);
    assert_eq!(d2.rows[0].result.participant, who("bob"));
    assert_eq!(d2.rows[0].result.turnover, Money::ZERO);
    assert_eq!(d2.rows[1].result.participant, who("alice"));
}

#[test]
fn a_participant_who_never_trades_is_listed_but_never_placed() {
    let mut h = Harness::new();
    h.buy("alice", 1, 100);
    h.mark("12");
    h.run(Command::CloseDay {
        day: day("2026-08-28"),
    });

    let board = h.engine.leaderboard(day("2026-08-28")).unwrap();
    let bob = board
        .rows
        .iter()
        .find(|r| r.result.participant == who("bob"))
        .unwrap();
    assert!(!bob.result.active);
    assert_eq!(bob.result.daily_return, Decimal::ZERO);

    let ladder = h.engine.ladder().unwrap();
    let bob = ladder.iter().find(|r| r.participant == who("bob")).unwrap();
    assert!(!bob.eligible);
    assert_eq!(bob.rank, None);
}

#[test]
fn closing_days_out_of_order_is_refused() {
    // Each day's return is measured against the *previous* close. Closing
    // 08-29 and then 08-28 would measure the earlier day against the later
    // one's baseline, and the ladder — which compounds in date order — would
    // chain two returns that were never computed against each other.
    let mut h = Harness::new();
    h.buy("alice", 1, 100);
    h.mark("12");
    h.run(Command::CloseDay {
        day: day("2026-08-29"),
    });

    assert!(
        h.try_run(Command::CloseDay {
            day: day("2026-08-28")
        })
        .is_err(),
        "a day before the latest close must be refused"
    );
}

#[test]
fn a_day_with_no_participants_cannot_be_closed() {
    // The command would otherwise journal a DayClosed carrying no entries, and
    // the leaderboard read that follows would then fail — a command that
    // succeeded and a response that did not.
    let mut e = Engine::new(MockBroker::simple(0));
    assert!(e
        .execute(
            Timestamp::from_millis(1),
            Command::CloseDay {
                day: day("2026-08-28")
            }
        )
        .is_err());
    assert!(
        e.closed_days().next().is_none(),
        "nothing should be journalled"
    );
}
