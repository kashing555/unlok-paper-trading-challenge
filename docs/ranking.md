# Ranking methodology

> The brief names ranking as an explicitly scored decision — *"you should decide
> and document how daily winners are ranked, how ties are resolved, whether the
> overall ladder is cumulative P&L / percentage return / ranking points, and how
> inactive participants are handled. The ranking should be deterministic and
> fair between participants."* This is that answer, with what each choice was
> made over.

## 1. Daily leaderboard — ranked on daily return %

```
daily_return = (closing_value − prior_closing_value) / prior_closing_value
```

where a participant's first day uses their **starting cash** as the prior close.

**Chosen over absolute daily P&L**, which is only fair if every participant
holds identical capital. The moment capital differs — a late joiner, a
differently-funded account, or simply a participant who compounded a good week —
absolute P&L ranks the largest book rather than the best trading. Return % is
capital-neutral, which is exactly the fairness property the brief asks for.

**Edge case: a wiped-out participant.** If `prior_closing_value <= 0` the return
is defined as 0% and the participant is flagged `bust`. Long-only with no
leverage means value cannot go negative, but it can approach zero, and a
division that can produce infinity has no place in a ranking.

## 2. Tie resolution — a total order, three levels deep

Ties on a return figure are not rare: two participants who both sat in cash both
score exactly 0.000000%. The sort must still produce one order, every time.

| # | Key | Direction | Argument |
|---|---|---|---|
| 1 | daily return % | desc | the result itself |
| 2 | gross traded notional (turnover) | **asc** | same return with less trading is the better result — less fee drag, less market exposure, better risk-adjusted |
| 3 | `participant_id` | asc | **total-order guarantee** |

Level 3 is arbitrary and openly so. Its job is not fairness but **determinism**:
with it, no two participants can compare equal, so the ranking cannot permute
between runs, across machines, or under a different input order. A ranking whose
output depends on `HashMap` iteration order is not deterministic, and the brief
asks for deterministic.

Level 2 is where the real judgment sits. Two participants finish the day both up
1.2%; one turned over $1M of notional to get there and the other turned over
$40k. The second took less risk for the same outcome. That is the better trader,
and the tiebreak says so.

## 3. Overall ladder — geometric compound of daily returns

```
ladder_return = Π (1 + daily_return_i) − 1        over all closed days
```

**Chosen over cumulative P&L.** Cumulative P&L can be bought with size: a
participant funded at 10× ranks above a better trader with a tenth of the
capital, permanently, regardless of skill. It measures the account, not the
participant.

**Chosen over the arithmetic sum of daily returns.** Returns chain
multiplicatively, not additively. Summing them overstates: −50% then +50% sums
to 0% but leaves you down 25%, and the ladder would report a participant who
lost a quarter of their capital as flat. Geometric compounding is not a
refinement here, it is the correct operation.

**Chosen over daily ranking points** (Formula-1 style: 25 for a win, 18 for
second, …). Points reward consistency and are robust to a single lucky day,
which is genuinely attractive. They were rejected as *primary* because they
discard magnitude — winning a day by 0.01% scores identically to winning it by
40% — and because a day's points depend on how many participants happened to be
active, so the same performance scores differently on different days.

They are, however, a real and defensible view of the competition, so **ranking
points are computed and exposed as a secondary column** on the ladder rather
than thrown away. The primary sort is the compound return; a reader who prefers
consistency can see both. Showing the alternative next to the choice is cheaper
than arguing about it.

### Ladder ties

| # | Key | Direction | Argument |
|---|---|---|---|
| 1 | cumulative geometric return | desc | the result |
| 2 | daily wins | desc | consistency over a single outlier day |
| 3 | active days | desc | breaks toward the participant who actually competed |
| 4 | `participant_id` | asc | total-order guarantee |

## 4. Inactive participants

**Definition.** A day is **active** for a participant if they had at least one
fill that day, or held a non-zero position at any point during it. Submitting
and cancelling without ever trading is not participation; carrying a position
from a prior day is, because it carries real exposure.

**The problem to decide on purpose.** Under return ranking, a participant who
does nothing scores exactly 0%. On a down day that beats every participant who
traded and lost. Left alone, an account that never places an order can top the
ladder in a falling market — a result nobody would defend as a *trading*
competition outcome.

**The decision, in two parts:**

1. **An inactive *day* ranks normally at 0%.** Choosing not to trade is a real
   trading decision, and cash is a position. A participant who reads the day as
   dangerous and stays flat has earned the 0%, and penalising it would be the
   competition dictating strategy.
2. **Ladder placement requires eligibility: at least one active day.** A
   participant with no activity anywhere in the competition is listed with
   `rank: null` and `eligible: false`, and is skipped when placement numbers are
   assigned. They are shown, not hidden — but they cannot win a trading
   competition without having traded.

The line is between *choosing to be flat on a day* (a strategy, ranked) and
*never having participated at all* (not a competitor, unranked).

## 5. Determinism

The brief asks for deterministic; that is a property we can assert, so it gets a
test rather than a claim.

- **Total order.** Every sort ends in `participant_id`. No two rows compare
  equal, so no ordering is left to the sort's discretion.
- **No float in a ranking input.** Values and P&L are integer minor units.
  `daily_return` is a `Decimal` at fixed scale, computed once from integer
  inputs and never re-derived through a float.
- **No hash iteration.** Participants are collected into a `Vec` and sorted
  explicitly; a `HashMap` is never iterated into an ordered output.
- **Closed days are immutable.** A published leaderboard is a snapshot. Later
  events — a late execution report, a corrected mark — do not retroactively
  change a day already closed, because a ranking that silently changes after
  publication is worse than one that is slightly stale.
- **The test:** shuffle the participant input order, recompute, assert the
  ranking is byte-identical.

## 6. Worked example

Three participants, all starting at $100,000. A and B trade actively; C buys
once on day 1 into a name whose mark then does not move, and holds.

| | Start | D1 close | D1 ret | D1 turnover | D2 close | D2 ret |
|---|---|---|---|---|---|---|
| **A** | 100,000 | 102,000 | +2.00% | 480,000 | 99,960 | −2.00% |
| **B** | 100,000 | 102,000 | +2.00% | 95,000 | 101,000 | −0.98% |
| **C** | 100,000 | 100,000 | 0.00% | 50,000 | 100,000 | 0.00% |

C is **active on both days** — a fill on day 1, and a non-zero position held
through day 2 — which is what makes the day-2 result below legitimate.

**Day 1 leaderboard.** A and B tie at +2.00%; turnover breaks it — B reached the
same return on $95k of trading against A's $480k.

1. B (+2.00%, turnover 95,000) 2. A (+2.00%, turnover 480,000) 3. C (0.00%)

**Day 2 leaderboard.** C's book is unchanged on a down day and wins it —
correctly: C is an active participant holding a real position that happened not
to move, not an empty account riding out the market.

1. C (0.00%) 2. B (−0.98%) 3. A (−2.00%)

**Ladder after day 2**, compounding:

| | Compound | Wins | Rank |
|---|---|---|---|
| **B** | 1.0200 × 0.9902 − 1 = **+1.00%** | 1 | 1 |
| **C** | 1.0000 × 1.0000 − 1 = **0.00%** | 1 | 2 |
| **A** | 1.0200 × 0.9800 − 1 = **−0.04%** | 0 | 3 |

Note A: up 2% then down 2% lands at **−0.04%**, not zero. That is the geometric
argument in one row — an additive ladder would have reported A as flat.

Had C never traded at all — no fill on any day and no position ever held — C
would instead be listed `eligible: false`, `rank: null`, and B and A would take
ranks 1 and 2. That is the line: *choosing to be flat* is a strategy and ranks;
*never having participated* is not, and does not.

## 7. Assumptions and limitations

- **All participants start together** with equal capital. Compounding a
  late joiner over fewer days is not directly comparable to a full-competition
  return. A per-day geometric *mean* would normalise this; it was rejected as
  unnecessary complexity under the stated assumption, and is the first thing to
  change if staggered entry is ever allowed.
- **No deposits or withdrawals after creation.** Daily P&L as a value difference
  is only valid without external cash flows; supporting them requires a
  time-weighted return with the flows subtracted out.
- **One currency, no financing, no dividends or corporate actions.** Each would
  otherwise land in daily P&L and distort the return.
- **Turnover as a tiebreak rewards low churn**, which is the right default for a
  competition scored on return. It would be the wrong default for one scored on
  liquidity provision.
