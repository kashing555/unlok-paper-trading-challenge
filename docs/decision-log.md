# Decision log

*Append-only. Each entry: what was decided, what it was chosen over, and the
argument that decided it. Newest at the bottom. Never rewritten — the topical
files in `docs/` are current truth; this is history.*

## 2026-08-28 — foundation

**Stack: Rust + Axum backend, Vue 3 + TS + Vite cockpit, SQLite store.**
Chosen over Python/FastAPI (faster to write, lower ceiling) and Java/Spring
(most boilerplate per unit of demonstrated judgment). The argument: patterns are
liftable from `sigma` — the same Axum gateway shape, the same store idiom, the
same pure-logic-in-a-tested-crate discipline — so the reference implementation
already exists and every line is defensible in a live interview. The cost is
time, accepted knowingly. Python remains the de-risk if the schedule slips.

**The execution report is the source of truth.** Position, cash, average cost
and P&L are folds over an append-only event log, never independently maintained
beliefs kept in sync alongside it. Chosen over mutable position rows updated per
fill — less code on day one, and the exact shape that produces silent
divergence. Every position bug worth the name comes from two copies of one fact
being allowed to disagree. Makes replay-determinism testable for free.

**Money is `i64` minor units behind newtypes; average cost is a rational.**
Chosen over `f64` and over storing a rounded average. A rounded average that is
re-multiplied drifts, and the drift only surfaces as a reconciliation that fails
weeks later. JSON carries money as decimal strings, not JSON numbers.

**Order lifecycle is an explicit transition table; illegal transitions are
rejected, not logged.** A fill on a terminal order is an error. Chosen over a
mutable status field, which cannot distinguish "impossible" from "unusual".

**Two ids: `client_order_id` minted before submit, `broker_order_id` recorded on
ack.** Chosen over keying orders by broker id, which cannot cancel inside the
submit→ack window and drops a fill when the ack races the execution report.

**Replace is cancel-replace, not in-place mutation.** Filled quantity survives on
the original; the residual is cancelled; a new order is minted with a `replaces`
link. A replace that loses the race to a complete fill is rejected rather than
silently creating an unwanted second order. Chosen over mutating price/qty,
which leaves no record of what was actually working when an execution report
arrives against the pre-modification terms.

**Concurrency: a single-writer command loop.** Chosen over per-participant
locking. Competition order volume fits one core comfortably, and the property
that matters is a single total replayable ordering of events, not throughput.
Interleaved writers make "same input, same leaderboard" a hope rather than a
test. Wrong trade for a matching engine, right one here — flagged as a
production delta in the README.

**Storage: SQLite, append-only `events`, projections replayed on boot.** Chosen
over an ORM with mutable rows (see above) and over pure in-memory — durability
costs one table and makes replay determinism assertable. Postgres buys nothing
an exercise this size can demonstrate.

**Fees capitalised into basis on buy, expensed on sell.** One convention applied
to both sides. The trap is applying it on one side only, which leaks a fee per
round trip into unrealized P&L.

**No mark, no value — day close fails closed.** A held symbol without a price
update is an error at close, not a silent zero. A wrong portfolio value corrupts
a leaderboard that is then immutable.

**Closed days are immutable and closing is idempotent.** Re-closing returns the
existing snapshot. Late events do not retroactively change a published ranking.

**Resting orders carry over across a day close.** Not cancelled — a working
order is exposure the participant intends to have, and cancelling it would be
the system making a trading decision on their behalf.

## 2026-08-28 — ranking

Full reasoning in [`ranking.md`](ranking.md). The decisions:

**Daily leaderboard ranks on daily return %,** not absolute P&L. Absolute P&L
ranks the largest book rather than the best trading the moment capital differs.

**Daily ties: return % → turnover ascending → `participant_id`.** Turnover is the
substantive level — the same return on less trading is the better result. The
`participant_id` level is arbitrary and openly so; its job is the **total-order
guarantee** that makes the output invariant to input order.

**Overall ladder is the geometric compound of daily returns.** Chosen over
cumulative P&L (buyable with size), over the arithmetic sum of returns
(−50% then +50% sums to 0% but leaves you down 25%), and over F1-style ranking
points as *primary* (discards magnitude; a day's points depend on how many
participants were active). Ranking points are computed and shown as a secondary
column rather than discarded — cheaper to display the alternative than to argue
about it.

**Inactive: an inactive day ranks normally at 0%, but ladder placement requires
at least one active day.** Staying flat is a real trading decision and cash is a
position, so a flat day earns its 0% and may win a down day. Never having traded
at all is not participation — those participants are listed with `rank: null`,
`eligible: false`, so an account that never places an order cannot top a trading
competition in a falling market. Active day := ≥1 fill, or a non-zero position
held at any point that day.

**Determinism is tested, not asserted:** shuffled input must produce a
byte-identical ranking; no float in a ranking input; no `HashMap` iterated into
an ordered output.

## 2026-08-28 — scope

**The Vue cockpit is beyond the brief** — *"no user interface is required"* — and
is built anyway, by operator decision, after the scored work is complete. It is
labelled as extra in the README and is never a dependency of anything the brief
asked for: the service must stay complete and demonstrable with `ui/` deleted.

**Explicitly not building:** shorts and margin · an order book with price-time
priority · limit orders resting against a live market · multi-currency ·
corporate actions · auth · rate limiting · horizontal scale. Each is stated in
the README so it reads as a decision rather than a gap.
