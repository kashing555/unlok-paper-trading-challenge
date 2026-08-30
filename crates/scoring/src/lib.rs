//! Daily results, the daily leaderboard, and the overall ladder.
//!
//! The full reasoning — what each choice was made *over* — is in
//! `docs/ranking.md`. In brief:
//!
//! - **Daily rank is on return %**, not absolute P&L, so participants with
//!   different capital compare fairly.
//! - **The ladder compounds daily returns geometrically**, because returns
//!   chain multiplicatively: −50% then +50% *sums* to zero but leaves you down
//!   25%.
//! - **Every sort ends in `participant_id`**, giving a total order, so no two
//!   rows can compare equal and no ranking can permute between runs.
//! - **An inactive day ranks normally at 0%** — staying flat is a decision —
//!   but **ladder placement requires at least one active day**, so an account
//!   that never traded cannot win a trading competition.
//!
//! Pure: no I/O, no clock, no randomness.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use domain::{Money, ParticipantId, TradingDay};
use rust_decimal::Decimal;
use thiserror::Error;

/// Decimal places a return is rounded to before it is compared or reported.
///
/// Fixed so that two runs cannot disagree in the last place, and so a rendered
/// figure is the same one that was ranked. Ten places is far finer than any
/// tiebreak needs and coarse enough to be stable.
pub const RETURN_SCALE: u32 = 10;

/// Points for a daily placing, Formula-1 style. Exposed as a **secondary** view
/// on the ladder — see `docs/ranking.md` §3 for why it is not the primary sort.
const POINTS: [u32; 10] = [25, 18, 15, 12, 10, 8, 6, 4, 2, 1];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScoringError {
    #[error("day {day} has no participants to rank")]
    NoParticipants { day: TradingDay },

    #[error("participant {0} appears twice in one day")]
    DuplicateParticipant(ParticipantId),
}

/// What the engine hands scoring at the end of a day.
///
/// `turnover` and `active` are facts about the day's fills, which the caller
/// knows and this crate does not — keeping the ranking a pure function of
/// numbers rather than of an event log it would have to re-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayInput {
    pub participant: ParticipantId,
    pub closing_value: Money,
    /// Yesterday's close, or starting cash on a participant's first day.
    pub prior_closing_value: Money,
    /// Gross notional traded today. The daily tiebreak.
    pub turnover: Money,
    /// At least one fill today, or a non-zero position held at any point.
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyResult {
    pub participant: ParticipantId,
    pub closing_value: Money,
    pub daily_pnl: Money,
    pub daily_return: Decimal,
    pub turnover: Money,
    pub active: bool,
    /// Prior value was zero or less, so a return is undefined and reported as
    /// zero. Long-only and unlevered, so this means wiped out, not negative.
    pub bust: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderboardRow {
    pub rank: u32,
    pub result: DailyResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leaderboard {
    pub day: TradingDay,
    pub rows: Vec<LeaderboardRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderRow {
    /// `None` when the participant has never been active — listed, but not
    /// placed. See `docs/ranking.md` §4.
    pub rank: Option<u32>,
    pub participant: ParticipantId,
    pub cumulative_return: Decimal,
    pub daily_wins: u32,
    pub active_days: u32,
    /// Secondary view only; never the sort key.
    pub points: u32,
    pub eligible: bool,
}

/// `(close − prior) / prior`, rounded to [`RETURN_SCALE`].
///
/// A prior value of zero or less yields zero rather than an infinity: a
/// division that can produce one has no place in a ranking.
fn ret(pnl: Money, base: Money) -> Decimal {
    if base.raw() <= 0 {
        return Decimal::ZERO;
    }
    (Decimal::from(pnl.raw()) / Decimal::from(base.raw())).round_dp(RETURN_SCALE)
}

/// Close a day: turn raw values into results. Row order follows input order;
/// the *ranked* output downstream is input-order-independent (tested).
pub fn daily_results(inputs: Vec<DayInput>) -> Result<Vec<DailyResult>, ScoringError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(inputs.len());

    for input in inputs {
        if !seen.insert(input.participant.clone()) {
            return Err(ScoringError::DuplicateParticipant(input.participant));
        }
        let daily_pnl = input
            .closing_value
            .checked_sub(input.prior_closing_value)
            .unwrap_or(Money::ZERO);

        out.push(DailyResult {
            daily_return: ret(daily_pnl, input.prior_closing_value),
            bust: input.prior_closing_value.raw() <= 0,
            participant: input.participant,
            closing_value: input.closing_value,
            daily_pnl,
            turnover: input.turnover,
            active: input.active,
        });
    }
    Ok(out)
}

/// Rank a day.
///
/// Sort keys, in order: **return % descending**, then **turnover ascending**
/// (the same return on less trading is the better result — less fee drag, less
/// exposure), then **`participant_id` ascending**.
///
/// That last key is arbitrary and openly so. Its job is not fairness but the
/// **total-order guarantee**: with it, no two participants compare equal, so
/// the output cannot depend on input order.
pub fn leaderboard(
    day: TradingDay,
    mut results: Vec<DailyResult>,
) -> Result<Leaderboard, ScoringError> {
    if results.is_empty() {
        return Err(ScoringError::NoParticipants { day });
    }

    results.sort_by(|a, b| {
        b.daily_return
            .cmp(&a.daily_return)
            .then(a.turnover.cmp(&b.turnover))
            .then(a.participant.cmp(&b.participant))
    });

    let rows = results
        .into_iter()
        .enumerate()
        .map(|(i, result)| LeaderboardRow {
            #[allow(clippy::cast_possible_truncation)]
            rank: i as u32 + 1,
            result,
        })
        .collect();

    Ok(Leaderboard { day, rows })
}

/// Build the overall ladder from every closed day, oldest first.
///
/// Cumulative return is `Π(1 + rᵢ) − 1`. Sort keys: **compound return
/// descending**, then **daily wins**, then **active days**, then
/// `participant_id`.
///
/// Placement numbers are assigned only to **eligible** participants — those
/// with at least one active day. Ineligible ones are listed with `rank: None`
/// rather than hidden: shown, but unable to win a trading competition without
/// having traded.
pub fn ladder(history: &[Leaderboard]) -> Vec<LadderRow> {
    use std::collections::BTreeMap;

    struct Acc {
        factor: Decimal,
        wins: u32,
        active_days: u32,
        points: u32,
    }

    let mut acc: BTreeMap<ParticipantId, Acc> = BTreeMap::new();

    for board in history {
        for row in &board.rows {
            let entry = acc.entry(row.result.participant.clone()).or_insert(Acc {
                factor: Decimal::ONE,
                wins: 0,
                active_days: 0,
                points: 0,
            });
            entry.factor *= Decimal::ONE + row.result.daily_return;
            if row.rank == 1 {
                entry.wins += 1;
            }
            if row.result.active {
                entry.active_days += 1;
            }
            entry.points += POINTS.get(row.rank as usize - 1).copied().unwrap_or(0);
        }
    }

    let mut rows: Vec<LadderRow> = acc
        .into_iter()
        .map(|(participant, a)| LadderRow {
            rank: None,
            participant,
            cumulative_return: (a.factor - Decimal::ONE).round_dp(RETURN_SCALE),
            daily_wins: a.wins,
            active_days: a.active_days,
            points: a.points,
            eligible: a.active_days > 0,
        })
        .collect();

    rows.sort_by(|x, y| {
        // Ineligible participants sort last regardless of return, then by the
        // same keys, so the listing is still deterministic among them.
        y.eligible
            .cmp(&x.eligible)
            .then(y.cumulative_return.cmp(&x.cumulative_return))
            .then(y.daily_wins.cmp(&x.daily_wins))
            .then(y.active_days.cmp(&x.active_days))
            .then(x.participant.cmp(&y.participant))
    });

    let mut place = 0;
    for row in &mut rows {
        if row.eligible {
            place += 1;
            row.rank = Some(place);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::FromPrimitive;

    fn who(n: &str) -> ParticipantId {
        ParticipantId::parse(n).unwrap()
    }
    fn money(s: &str) -> Money {
        Money::parse(s).unwrap()
    }
    fn day(s: &str) -> TradingDay {
        TradingDay::parse(s).unwrap()
    }
    fn dec(f: f64) -> Decimal {
        Decimal::from_f64(f).unwrap().round_dp(RETURN_SCALE)
    }

    fn input(name: &str, close: &str, prior: &str, turnover: &str, active: bool) -> DayInput {
        DayInput {
            participant: who(name),
            closing_value: money(close),
            prior_closing_value: money(prior),
            turnover: money(turnover),
            active,
        }
    }

    fn board(d: &str, inputs: Vec<DayInput>) -> Leaderboard {
        leaderboard(day(d), daily_results(inputs).unwrap()).unwrap()
    }

    fn order_of(b: &Leaderboard) -> Vec<&str> {
        b.rows
            .iter()
            .map(|r| r.result.participant.as_str())
            .collect()
    }

    /// Day 1 and day 2 of the worked example in `docs/ranking.md` §6. If the doc
    /// and the code ever disagree, this fails — which is the point of writing
    /// the example down with numbers in it.
    fn worked_example() -> [Leaderboard; 2] {
        [
            board(
                "2026-08-28",
                vec![
                    input("alice", "102000", "100000", "480000", true),
                    input("bob", "102000", "100000", "95000", true),
                    input("carol", "100000", "100000", "50000", true),
                ],
            ),
            board(
                "2026-08-29",
                vec![
                    input("alice", "99960", "102000", "300000", true),
                    input("bob", "101000", "102000", "60000", true),
                    input("carol", "100000", "100000", "0", true),
                ],
            ),
        ]
    }

    #[test]
    fn the_worked_example_day_one_breaks_a_tie_on_turnover() {
        let [d1, _] = worked_example();
        // alice and bob both +2.00%; bob reached it on 95k of trading against
        // alice's 480k, so bob takes the day.
        assert_eq!(order_of(&d1), ["bob", "alice", "carol"]);
        assert_eq!(d1.rows[0].result.daily_return, dec(0.02));
        assert_eq!(d1.rows[1].result.daily_return, dec(0.02));
        assert_eq!(d1.rows[0].result.daily_pnl, money("2000"));
    }

    #[test]
    fn the_worked_example_day_two_is_won_by_the_unchanged_book() {
        let [_, d2] = worked_example();
        assert_eq!(order_of(&d2), ["carol", "bob", "alice"]);
        assert_eq!(d2.rows[2].result.daily_return, dec(-0.02));
        // -1000/102000 = -0.0098039215..., rounded to RETURN_SCALE
        assert_eq!(d2.rows[1].result.daily_return, dec(-0.0098039216));
    }

    #[test]
    fn the_worked_example_ladder_compounds_rather_than_sums() {
        let l = ladder(&worked_example());
        let by = |n: &str| {
            l.iter()
                .find(|r| r.participant.as_str() == n)
                .unwrap()
                .clone()
        };

        // bob: 1.02 x 0.9901960784 - 1 = +1.00%
        assert_eq!(by("bob").cumulative_return, dec(0.01));
        assert_eq!(by("carol").cumulative_return, Decimal::ZERO);
        // alice: up 2% then down 2% is NOT flat. An additive ladder would have
        // reported 0.00% and been wrong.
        assert_eq!(by("alice").cumulative_return, dec(-0.0004));

        assert_eq!(
            l.iter().map(|r| r.participant.as_str()).collect::<Vec<_>>(),
            ["bob", "carol", "alice"]
        );
        assert_eq!(
            l.iter().map(|r| r.rank).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3)]
        );
        assert_eq!(by("bob").daily_wins, 1);
        assert_eq!(by("carol").daily_wins, 1);
        assert_eq!(by("alice").daily_wins, 0);
        // Points are the secondary view: bob 25 + 18, carol 15 + 25, alice 18 + 15.
        assert_eq!(
            (by("bob").points, by("carol").points, by("alice").points),
            (43, 40, 33)
        );
    }

    #[test]
    fn a_geometric_ladder_reports_the_loss_an_additive_one_would_hide() {
        // -50% then +50% sums to zero and compounds to -25%. This single case is
        // the whole argument for the geometric form.
        let history = [
            board(
                "2026-08-28",
                vec![
                    input("alice", "50000", "100000", "0", true),
                    input("bob", "100000", "100000", "0", true),
                ],
            ),
            board(
                "2026-08-29",
                vec![
                    input("alice", "75000", "50000", "0", true),
                    input("bob", "100000", "100000", "0", true),
                ],
            ),
        ];
        let l = ladder(&history);
        let alice = l
            .iter()
            .find(|r| r.participant.as_str() == "alice")
            .unwrap();
        assert_eq!(alice.cumulative_return, dec(-0.25));
        // and so alice ranks below the participant who did nothing
        assert_eq!(l[0].participant.as_str(), "bob");
    }

    #[test]
    fn ranking_is_invariant_to_input_order() {
        // The determinism the brief asks for, asserted rather than claimed.
        let make = |names: [&str; 3]| {
            board(
                "2026-08-28",
                names
                    .iter()
                    .map(|n| input(n, "100000", "100000", "0", true))
                    .collect(),
            )
        };
        let a = make(["alice", "bob", "carol"]);
        let b = make(["carol", "alice", "bob"]);
        let c = make(["bob", "carol", "alice"]);
        assert_eq!(a, b);
        assert_eq!(b, c);
        // Fully tied on return and turnover, so participant id decides — and it
        // always can, because it is unique.
        assert_eq!(order_of(&a), ["alice", "bob", "carol"]);
    }

    #[test]
    fn an_inactive_day_ranks_normally_at_zero_percent() {
        // Staying flat is a trading decision, and on a down day it wins.
        let b = board(
            "2026-08-28",
            vec![
                input("alice", "95000", "100000", "500000", true),
                input("bob", "100000", "100000", "0", false),
            ],
        );
        assert_eq!(order_of(&b), ["bob", "alice"]);
        assert_eq!(b.rows[0].result.daily_return, Decimal::ZERO);
        assert!(!b.rows[0].result.active);
    }

    #[test]
    fn a_participant_who_never_traded_is_listed_but_never_placed() {
        let history = [board(
            "2026-08-28",
            vec![
                input("alice", "95000", "100000", "500000", true),
                input("ghost", "100000", "100000", "0", false),
            ],
        )];
        let l = ladder(&history);

        let ghost = l
            .iter()
            .find(|r| r.participant.as_str() == "ghost")
            .unwrap();
        assert!(!ghost.eligible);
        assert_eq!(
            ghost.rank, None,
            "an account that never traded cannot place"
        );
        // ...even though its 0% beat alice's -5% on the day.
        assert_eq!(ghost.cumulative_return, Decimal::ZERO);

        let alice = l
            .iter()
            .find(|r| r.participant.as_str() == "alice")
            .unwrap();
        assert_eq!(alice.rank, Some(1));
        // Ineligible rows sort last, so placement numbers stay contiguous.
        assert_eq!(l.last().unwrap().participant.as_str(), "ghost");
    }

    #[test]
    fn one_active_day_is_enough_to_be_eligible_forever_after() {
        let history = [
            board(
                "2026-08-28",
                vec![input("alice", "100000", "100000", "10", true)],
            ),
            board(
                "2026-08-29",
                vec![input("alice", "100000", "100000", "0", false)],
            ),
        ];
        let row = &ladder(&history)[0];
        assert!(row.eligible);
        assert_eq!(row.active_days, 1);
        assert_eq!(row.rank, Some(1));
    }

    #[test]
    fn a_wiped_out_participant_yields_zero_rather_than_an_infinity() {
        let results = daily_results(vec![input("alice", "0", "0", "0", true)]).unwrap();
        assert_eq!(results[0].daily_return, Decimal::ZERO);
        assert!(results[0].bust);
    }

    #[test]
    fn a_day_with_no_participants_or_a_repeated_one_is_rejected() {
        assert_eq!(
            leaderboard(day("2026-08-28"), vec![]),
            Err(ScoringError::NoParticipants {
                day: day("2026-08-28")
            })
        );
        assert_eq!(
            daily_results(vec![
                input("alice", "1", "1", "0", true),
                input("alice", "2", "1", "0", true),
            ]),
            Err(ScoringError::DuplicateParticipant(who("alice")))
        );
    }

    #[test]
    fn ladder_ties_fall_through_wins_then_active_days_then_id() {
        // Identical returns; bob won a day, so bob is ahead on the second key.
        let history = [
            board(
                "2026-08-28",
                vec![
                    input("alice", "100000", "100000", "500", true),
                    input("bob", "100000", "100000", "100", true),
                ],
            ),
            board(
                "2026-08-29",
                vec![
                    input("alice", "100000", "100000", "500", true),
                    input("bob", "100000", "100000", "100", true),
                ],
            ),
        ];
        let l = ladder(&history);
        assert_eq!(l[0].participant.as_str(), "bob");
        assert_eq!(l[0].daily_wins, 2);
        assert_eq!(l[1].daily_wins, 0);
    }
}
