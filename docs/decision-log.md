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

## 2026-08-28 — structure

**Structural rules extracted into `.claude/principles.md`.** They were implicit
in `design.md`; making them a separate, citable file means a design change can be
checked against them rather than argued about. Cites the real names — ports and
adapters, functional core / imperative shell, single-writer, LMAX — because that
is vocabulary the reviewer already has, and being able to say which parts we
*declined* is the actual signal.

**Enforcement is the workspace, not discipline.** `domain` and `scoring` are
separate crates rather than modules specifically so that an inward-pointing
dependency rule is a **compile error**: a crate cannot import what is not in its
`Cargo.toml`. Chosen over one binary with module conventions, which relies on
discipline, and discipline is what erodes on day two.

**Coupling is checked, not felt.** Five runnable tests — deletion, swap, purity,
parallel-work, explanation (`principles.md` §4). The deletion test is what backs
the README's claim that the service is complete with `ui/` deleted; it is a
property we can run rather than a promise.

**SOLID applied selectively, and the declines are documented.** SRP and ISP hard;
DIP only at real seams (`store`, `broker`); **OCP mostly declined** — extension
points for imagined variation are `baseline.md` §2's speculative flexibility
wearing a principle's name. The single exception is the ranking strategy, where
the brief itself asks us to weigh alternatives, so a second implementation really
exists. Also declined: DI framework, generic `Repository<T>`, interface-per-
struct, mapper layers between identical structs.

**Mechanical sympathy is explicitly out of scope** — named so it reads as a
decision. A paper trading competition is not latency-bound; optimising cache
lines here would be the same speculative-complexity error as OCP above. First
thing to reach for if this ever became a real matching engine.

**Build order: the engine before everything else** (`docs/build-order.md`).
Stage A — money types, order lifecycle, position/P&L, mock broker, engine loop —
completes and is fully tested with no HTTP, no database and no clock, and there
is an explicit gate there. The argument is risk: if the schedule breaks at the
gate, what exists is a tested trading engine with a written design, which is a
defensible submission. The alternative ordering — scaffold the web service
first — fails to a half-wired CRUD app with the scored content missing.

**A1/A2 and B1/B2 are deliberately independent** so two people can work at once.
That independence is the parallel-work test from `principles.md` §4 being cashed
in, and it is the practical reason the dependency rule is worth enforcing at all.

## 2026-08-29 — type-driven design, coupling vocabulary, Rust structure

**Connascence adopted as the vocabulary for coupling** (`principles.md` §4).
"Coupling is bad" is not actionable; connascence grades it by strength, locality
and degree, and yields a rule that is: *the further apart two things sit, the
weaker their connascence must be.* The operative move for us is **converting
connascence of meaning into connascence of type** — a function taking
`(i64, i64)` where one is cents and the other a scaled price shares meaning with
every caller and cannot be checked; `(Cash, Px)` shares only type and the
compiler checks it. That is most of what the newtypes are for. It also renames
the §1 bug precisely: two copies of one fact is *connascence of value across a
boundary*, and dynamic connascence is worse than static because the compiler
cannot see it at all.

**Type-driven design promoted to its own part** (`principles.md` §3): make
illegal states unrepresentable, and parse rather than validate. Both are the
same move — push the guarantee into the type so the compiler maintains it — and
the same trade as enforcing the dependency rule with crates rather than
convention.

**The order lifecycle is a runtime enum, not typestate** (`rust.md`). Typestate
(`Order<New>`, `Order<Acknowledged>`) makes an illegal transition fail to
compile and was the attractive option. Rejected for one disqualifying reason:
**order state is loaded from an event log at runtime**, so the type to construct
is not known statically; a store would have to erase back to an enum at the
boundary, leaving both mechanisms and the guarantee of neither. Taken instead: a
total transition function with exhaustiveness enforced by `match` and **no
wildcard arm** — which still catches the mistake that actually happens (adding a
state and not handling every pairing). A wildcard in a transition match is a
review failure. Typestate stays right where the caller knows the state
statically; ours is runtime uncertainty.

**Postel's law rejected on every input path** (`principles.md` §7). "Be liberal
in what you accept" is how a malformed order becomes a real position. Missing
side, unparseable price, ambiguous quantity → **reject loudly at the boundary**,
never repair into something plausible. A rejected order costs a retry; an
accepted misinterpretation costs money and is found later. Named explicitly
because it is a famous principle and declining it needs to read as a decision.

**Also declined, with reasoning:** DRY pursued past knowledge into syntax
(duplication is cheaper than the wrong abstraction — two functions that look
alike but change for different reasons are not duplication); speculative
generality; layers that only forward; blanket defensive copying.

**CUPID cited alongside SOLID** (`principles.md` §5) as the more useful lens at
this size — properties with a direction of travel rather than rules. *Predictable*
is not a nicety here: it is the determinism the brief actually requires.

**Serde does not touch the domain** (`rust.md`). DTOs live in `api` with explicit
`From` impls. A wire format is a contract with the outside world; deriving it on
domain types couples the two, makes an internal rename a breaking API change,
and invites unintended fields into the JSON. It is also what lets money
serialise as a decimal string at the edge while staying `i64` inside. The extra
`From` impls are the price and they are the seam.

**Errors split by kind, not severity** (`rust.md`): a rejected order is a
`Result` (a normal competition outcome); a negative position is a **bug** —
`debug_assert!` plus a hard error, never a warning. `thiserror` in libraries so
callers can match, `anyhow` in binaries only. `unwrap_used`/`expect_used` denied
in `domain` by workspace lint rather than by review.
