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

## 2026-08-29 — delivery discipline, and a stop on documentation

**The git history is treated as part of the submission** (`delivery.md`). The
reviewer runs `git log`; it is the only record of *how* this was built. Atomic
commits, every one compiling with tests green, messages carrying the why — and
a history that shows the engine landing before any HTTP, demonstrating the
build-order argument rather than asserting it.

**A fixed cut order, decided now rather than at midnight on day two:** cockpit,
then CLI, then polish. Never tests, never the README, never anything in Stage A.
Cut from the end, never the middle — a missing layer is a documented decision, a
half-wired one is a bug and reads as one. Cut scope, never rigour. Everything cut
goes in the README's limitations section with its reasoning; owning a gap costs
nothing, being caught with an unmentioned one costs everything.

**"Write nothing we cannot defend"** promoted to an explicit rule, since the
brief makes it a constraint (*"you should be able to explain and modify all
submitted code"*) and it binds hardest on generated code. Every dependency
justifiable in a sentence; every abstraction able to answer "what breaks if this
is deleted?"; generated code read line by line before commit, at the same
standard as typed code.

**Testing discipline: fakes over mock frameworks** (`rust.md`). Ports are small,
so an in-memory `FakeEventLog` is a dozen lines and behaves like the real thing.
A mock asserting which methods were called in what order tests the
implementation — connascence of algorithm between test and code — so every
refactor breaks it. Also: assert on state and events, never private fields or
call counts; and **do not test the compiler** — there is no test to write for
"a `Qty` cannot be negative" when the constructor makes it unrepresentable. Test
the parsing boundary instead.

**Documentation stops here.** 1,850 lines of design against zero lines of code
is itself the speculative-complexity failure these files warn about, and the
brief scores scope discipline. The design is settled enough to build from and
every remaining question is one that implementation answers better than more
prose. Next commit is Stage A0.

## 2026-08-29 — Stage A0 landed

**One shared money scale of 1e4, not cents.** `design.md` §3 originally said
minor units; the implementation proved that wrong and the doc was the bug (the
`docs/README.md` maintenance rule, first time it fired). With `Cash` and `Px` on
the *same* scale and `Qty` unscaled, `notional = px.raw × qty` is an exact
multiply — **no division, so no rounding anywhere in the core**. On a cents
scale it would have been `px_raw × qty / 100`, and `$10.0050 × 1 share` is
1000.5 cents: representable only by rounding, on every fill, silently. Rounding
to a currency's minor unit at settlement is a production concern (§16).

**Overflow is a `Result`, and there are no `Add`/`Sub` impls on money.**
Operator convenience would be paid for in a P&L that is wrong without saying so,
so arithmetic goes through `checked_*` and the caller handles it. Tested at
`i64::MAX`.

**`Qty` is non-negative, not strictly positive.** Zero is a real quantity — an
unfilled order has `filled == 0` and a closed position has `qty == 0`. The
stricter "an *order* must be for more than zero shares" rule belongs on the
order constructor (A1), so the errors are separate variants: `NegativeQty` is
the type invariant, `NonPositiveQty` the order rule. `Qty::checked_sub`
returning `NegativeQty` is what makes a short position unrepresentable rather
than merely forbidden.

**Ids are parsed into one canonical form and never repaired into one.**
`Symbol::parse("aapl")` is an **error**, not an upper-casing. This is the
double-counting lesson applied: a system that accepted two spellings of one
account key filed every execution twice and double-counted P&L that never
happened. Same rule gives `TradingDay` an explicit length check, since
`parse_from_str` would otherwise accept `2026-8-1` as a second spelling of one
day and key two leaderboards for the same date.

**The purity rule is compiler-enforced, and that was verified rather than
assumed.** `chrono` is declared `default-features = false, features = ["std"]`,
which drops the `clock` feature that provides `Utc::now()`. A probe confirms
`Utc::now()` inside `domain` fails to compile with `E0599`. `domain`'s entire
dependency tree is `chrono` + `thiserror`: no async runtime, HTTP, SQL, clock or
RNG, which is A0's stated close condition.

**A test caught a real parser bug before commit:** `"1."` parsed as `1.0000`
because `all(is_ascii_digit)` passes vacuously on an empty fraction. Rejected
now. This is the reject-don't-guess rule earning its place on day one — the
input was ambiguous and the parser was quietly resolving it.

**A0 is closed:** 18 tests, `clippy -D warnings` clean, `fmt` clean.

## 2026-08-29 — naming corrections

**`Cash` renamed to `Money`.** Raised in review and correct: the type was doing
duty for a cash balance, a notional, a fee and a P&L, but only the first of
those *is* cash. `Money` is the type; cash is one use of it — the participant's
uninvested balance, which becomes a `cash: Money` field on the portfolio. A fee
is not cash and an unrealized P&L certainly is not. Renamed now, while A0 is the
only thing depending on it and the change costs one commit.

**Two order ids kept, and the FIX mapping documented.** Also challenged in
review — "trading systems just use an order id". They do not: FIX carries both
`ClOrdID` (tag 11, assigned by us before sending) and `OrderID` (tag 37,
assigned by the broker), and a cancel/replace mints a *new* `ClOrdID` pointing
at the previous one through `OrigClOrdID` (tag 41), which is precisely the
`replaces` link in `design.md` §5. The same split exists in sigma as
`cloid`/`oid`.

The justification is not theoretical here. Because executions are driven as a
**separate operation** from submission (the brief lists "generate mock
executions" as its own interface), an order genuinely sits in `NEW` with no
broker id until the ack is generated. Cancelling in that window is a real,
reachable, tested state — and it is only expressible with an id we minted
ourselves. Keying the registry on the broker's id instead would also drop any
execution report arriving before the ack is processed, which is the classic
missed-fill path.

The tag numbers now sit in the doc comments, so the design is checkable against
the protocol rather than taken on trust.

## 2026-08-29 — where names come from

**Raised in review: "how did we decide all this naming, where is it referenced
from?" The gap was real** — the rule existed in my head and in one CUPID bullet,
but was never written down, so every name read as a personal preference. Now in
`code-style.md` with a full traceability table.

**The rule: the brief's word wins; where the brief is silent, FIX; where both
are silent, the standard pattern name.** Never invent a synonym for a term the
brief already uses, never abbreviate one.

Checked against the source: **`cash` is the brief's own word** ("cash balance",
`challenge.md` line 21), as are participant, position, average position price,
realized/unrealized P&L, total portfolio value, daily P&L, daily return
percentage, closing portfolio value, daily leaderboard, overall competition
ladder, and all six order states verbatim. `ClientOrderId`/`BrokerOrderId`/
`replaces` are FIX tags 11/37/41; `Px` and `Qty` are FIX's own abbreviations
(`LastPx` 31, `AvgPx` 6, `OrderQty` 38, `CumQty` 14, `LeavesQty` 151). `Money`
is Fowler's Money pattern.

**Two deviations, documented as deviations:** `Money` as the type where the brief
says cash (cash is one *use* of it; a fee is not cash — so the type is `Money`
and the field is `cash`), and `mark` for the valuation price where the brief says
"market price" (the endpoint keeps the brief's word, `POST /market/prices`;
the domain uses `mark` inside). Brief's language at the edge, domain language in
the core.

An undocumented deviation is indistinguishable from carelessness, which is the
whole reason this entry exists.

## 2026-08-29 — pattern lineage

**Raised in review: PoEAA is 2002 and Rust 1.0 is 2015 — do these patterns
actually fit?** Mostly they do not, and the ones that do not were already
declined; `rust.md` now says why rather than leaving it to inference.

**Kept, because they were never about objects:** *Money* (a value object with
exact arithmetic — Java needs final fields, hand-written `equals`/`hashCode` and
defensive copies to get it, and still cannot stop `money + price` compiling,
where a Rust newtype is zero-cost and rejects it at compile time — the pattern is
*stronger* here than where it was written); *event sourcing* (a fold over a log,
closer to `fold` than to a class); *single-writer/LMAX* (LMAX was Java fighting
the JVM, and the Disruptor's pre-allocated ring buffer exists largely to dodge GC
pauses — with no GC and deterministic destruction the architecture is easier in
Rust, not ported).

**Declined, because they solve problems Rust does not have:** Repository / Unit
of Work / Identity Map (machinery for a mutable object graph synced to rows — an
append-only log has no identity map problem); Active Record (a mutable object
that *is* a row, hiding I/O behind field access); Lazy Load (needs hidden
mutation behind a getter, which `&self` forbids); Null Object (a workaround for
`null`); Service Layer with a DI container (constructor injection substituting
for absent first-class functions and generics).

**The actual lineage of this codebase is the ML family, not enterprise OOP** —
ML meaning *Meta Language* (Milner, 1973) and its descendants OCaml, Standard ML
and F#, plus Haskell; **not machine learning**, an acronym collision worth
spelling out before a reviewer trips on it —
newtypes, parse-don't-validate, illegal-states-unrepresentable, exhaustive sum
types, functional core. Rust took its type system from that tradition, which is
why those transfer natively and the 2002 catalogue mostly does not.

## 2026-08-29 — Rust rules promoted into CLAUDE.md

The language-level rules were only in `rust.md`, one router hop away, while
`CLAUDE.md` carried the architecture rules directly. Anything that must never be
violated belongs where it is read first, so the eight non-negotiables are now in
`CLAUDE.md`: illegal states unrepresentable, parse-don't-validate, exhaustive
match with no wildcard arm, newtypes carrying units, errors-as-values with bug
and expected-failure distinguished, ownership over shared mutability, data and
functions rather than objects with behaviour, and `forbid(unsafe_code)`.

The framing carries the conclusion from the pattern-lineage discussion: **this is
not Java with a borrow checker.** Rust took sum types, exhaustive matching and
`Option`/`Result` from the ML family, so the design uses them the way that
tradition does — a state machine is an enum with a total transition function, not
a class hierarchy with a mutable status field. Reaching for `Rc<RefCell<_>>` to
model an object graph is the tell that an OOP shape is being forced, which is why
it is named explicitly rather than left to taste.

## 2026-08-29 — housekeeping

**Audit for stale and unrelated content, prompted by review.** No orphaned docs:
every file in `.claude/` is routed from `CLAUDE.md` and every file in `docs/` is
indexed in `docs/README.md`, both verified rather than assumed. Two real
problems found, both self-inflicted:

**`.gitignore` carried boilerplate for a stack we are not using** — Python
(`__pycache__`, `.venv`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`,
`*.egg-info`), Next.js (`.next/`) and Jupyter (`.ipynb_checkpoints/`), written
in the first commit before the stack was chosen. Reduced to what is actually
true: Rust `target/`, Node for the eventual `ui/`, the generated SQLite log,
secrets and editor files. An ignore file listing tooling the project does not use
is a small lie about what the project is, and it is the first file a reviewer
skims.

**`README.md` claimed "implementation not started"** while A0 was committed and
green. The most-read file in the repo was the one saying something false. Now
carries a stage table that tracks the build order, and is updated as stages
close rather than at the end.

**`Cargo.lock` is committed deliberately** — the workspace produces binaries
(`api`, `cli`), and a reviewer should get the exact dependency versions the tests
passed against.

## 2026-08-29 — CLAUDE.md describes the repository, not only the principles

Raised in review: does `CLAUDE.md` actually reflect how we want the repository to
be? It did not. It carried the persona, the architectural rules, the Rust rules
and the build order — but never said **what the repository is**, and had no rule
preventing the staleness found in the audit an hour earlier.

**Added: the crate layout**, with the one-line statement of the dependency
direction (`domain` depends on nothing, `api`/`cli` depend on everything, nothing
depends on them) and the note that it is a *compile error* when violated. Also
states that the layout lands stage by stage and is never scaffolded upfront —
empty crates waiting to be filled are speculative structure, which `principles.md`
§7 declines.

**Deliberately not restated: build progress.** `CLAUDE.md` points at the stage
table in `README.md` as the single place tracking it. Two copies of one fact is
the bug this system is designed around, and documentation is not exempt — the
README went stale precisely because progress lived in more than one head.

**Added: maintenance as rules rather than aspirations.** Close a stage → update
the stage table. Change a decision → append to the log *and* update the topical
file. A doc disagreeing with the code is the bug. **Re-audit for staleness at
each build-order gate**, checking rather than assuming that every `.claude/` file
is routed and every `docs/` file indexed.

The framing is what the audit taught: the failure mode here is not a bad
decision, it is a file that was correct when written and never revisited. A
reviewer reads the repository as evidence of how we work, and reads a stale file
as carelessness — not wrongly.

## 2026-08-29 — Stage A1 landed

**The transition function is total, and exhaustiveness is compiler-enforced —
verified by breaking it on purpose.** Adding a variant to `OrderState` or to
`OrderEvent` produces `E0004: non-exhaustive patterns` rather than compiling
into a silently unhandled case. That is the whole argument for the runtime-enum
decision (`rust.md`) paying off: the guarantee typestate would have given at the
type level is instead obtained from the match, in a shape that survives being
loaded from an event log.

The terminal states are matched as `Filled | Cancelled | Rejected` — a named
or-pattern, not a `_` arm — so they reject every event without switching the
check off.

**States carry exactly their own data.** `Acknowledged` cannot exist without a
broker id; `Rejected` has no filled quantity because there is no such thing.
`broker_id` is present only on the *live* states: it is needed to cancel an
order and to correlate reports against it, and once terminal that is history,
which lives in the event log rather than in the state.

**Orders accumulate `cost`, not an average price.** Two fills at 10.0050 and
10.0150 sum to exactly 30.0350; storing a rounded average and re-multiplying it
would drift, and the drift only surfaces as a reconciliation that fails days
later. This is `code-style.md`'s rational-not-rounded rule applied at the order
level, and the portfolio (A2) will do the same at the position level.

**Fees are not in the lifecycle.** `cost` is gross notional of fills. Fees belong
to the portfolio fold, which keeps A1 about *lifecycle* and A2 about
*accounting* — the two are independent stages precisely so they can be worked on
separately.

**Rejected: a fill on a terminal order, an empty fill, and an overfill.** Each is
an error rather than a warning-and-proceed. A fill arriving on a `FILLED` order
is not an anomaly to log, it is a P&L bug that would otherwise be found during a
reconciliation days later.

**Replace is cancel-replace and cannot rewrite history.** Replacing an order that
is 40/100 filled withdraws the residual 60; the 40 stays booked against the
original, and the replacement is a new order carrying `replaces` (FIX
`OrigClOrdID`). A replace that loses the race to a complete fill returns
`Illegal { state: "FILLED" }` rather than silently opening a second position the
participant never asked for — the error names the state so the caller can decide.

**A1 is closed:** 35 tests including the full 6 × 4 state/event matrix with a
length assertion that stops a new pair escaping coverage. `clippy -D warnings`
clean, `fmt` clean.

## 2026-08-29 — Stage A2 landed

**Average cost is never stored.** The position holds `qty` and `cost`, and the
average is derived only for display. The walkthrough test shows why it matters:
after buying 100 @ 10 and 100 @ 12 the average is 11.05, and **selling 50 leaves
it at 11.05** — the sale removes basis and quantity in the same proportion. A
stored, rounded average would have drifted at that step and again at every one
after it.

**Unrealized is computed as `qty × mark − cost`, not `qty × (mark − avg)`.**
Algebraically identical; the second form needs the average and therefore a
division, while the first is exact. Choosing the exact form is free.

**Partial sales truncate the basis they remove, and the residue cannot
accumulate.** `cost_removed = cost × sold / held`, widened to `i128` so the
multiply cannot overflow before the divide. The final sale of a position has
`sold == held`, so that division is exact and takes the whole remaining basis
with it. Tested on a basis that does not divide evenly (31.0000 over 3 shares):
the intermediate sale leaves 20.6667, and the close leaves **exactly zero**. A
`debug_assert!` guards the invariant in case that ever stops being true.

**Fees are capitalised into the basis on buy and expensed on sell.** One
convention, both sides, so a round trip does not leak a fee into unrealized P&L.
Visible in the tests: buying 100 @ 10 with a 5.00 fee gives a basis of 1005 and
an unrealized of −5.00 at a mark of 10, not zero.

**Rejected fills leave the book untouched.** Both the insufficient-cash and
insufficient-position paths validate *before* mutating, and both are asserted
against a full clone of the portfolio. A half-applied fill would be a position
that disagrees with the cash that paid for it — the divergence the whole design
exists to prevent.

**Insufficient cash is an error, not a negative balance.** It is reachable only
if a pre-trade check upstream failed, which makes it a bug, and bugs in an
accounting path fail loud (`principles.md` §6).

**Missing marks fail closed.** A held symbol with no price errors rather than
valuing at zero — a wrong portfolio value corrupts a leaderboard that is then
immutable. A flat book needs no marks at all, which the tests pin.

**Positions live in a `BTreeMap`.** Iteration order is a property of the symbols
rather than of a hash seed, which is one of the places determinism would
otherwise leak away silently. Asserted directly.

**The invariant test worth keeping:** `total_value == starting_cash +
realized_pnl + unrealized_pnl`, whatever route the fills took. It catches almost
any accounting slip in one assertion.

**A2 is closed:** 49 tests, `clippy -D warnings` clean, `fmt` clean.

## 2026-08-29 — Stage A3 landed

**The RNG is owned and seeded; `thread_rng` appears nowhere.** Same seed, same
executions, on every run and every machine — asserted directly, and asserted
again in the negative (different seeds diverge) so the test cannot pass by the
generator being ignored.

**Partial fills always terminate and always sum exactly.** Each execution takes
a seeded slice of what remains, and the `max_slices`-th one takes the rest, so
an order cannot be left working forever. Checked across 25 seeds on a quantity
that divides badly (997): the slices sum to 997 every time.

**Broker limits are broker-side only.** Unknown symbol and size cap live here;
**insufficient cash and insufficient position do not**, because the broker does
not know what a participant holds. The engine that does know checks them before
an order reaches the broker. Splitting rejections by who can actually see the
reason keeps the port small.

**Fees are basis points of notional, rounded down** — arbitrary, but *stated*.
The alternative is a fee that depends on a rounding rule nobody wrote down.

**No price improvement, no book, no queue position.** Fills are marketable-limit
at the order's own price. Named as an omission so it reads as a decision, and
because "the fill price is always the limit price" is a thing a reviewer will
otherwise ask about.

## 2026-08-29 — Stage A4 landed; the gate is passed

**`apply` is the only mutator, and replay uses the same path.** Anything
`decide` learns that is not written into an event would be lost on replay, so
broker outputs — the minted broker id, the chosen fill size — go **into the
event** rather than being applied directly. This makes "rebuild from the log and
get the same state" true by construction.

**Verified, not asserted: replay never consults the broker.** The replay test
rebuilds with a broker on a *different seed* and asserts the resulting snapshot
is identical to the live engine's, field for field, across portfolios, positions
and every order. If replay touched the broker at all, that test would diverge.

**Commands validate fully before anything mutates.** A fill is checked against
*both* the order's lifecycle and the book — the latter against a **clone of the
portfolio** — before an event is emitted. Cloning one portfolio is cheap, and it
is the difference between "the command was refused" and "the order advanced but
the cash did not". Asserted by taking a full snapshot before five different
refused commands and requiring it unchanged after each.

**Pre-trade reservations are derived, never cached.** Available cash is the
balance minus the notional of *working buy orders*, recomputed from those orders
every time. A reserved-cash counter kept alongside them would be a second copy
of a fact the orders already hold, and the two would eventually disagree — the
§1 bug. The test that pins it: with 1000 cash, one working order for 600 makes a
second 600 order refused *at submit*, rather than passing and failing deep in
the book at fill time, which is far too late.

**A replace releases the reservation it is about to cancel.** Otherwise
replacing an order that commits the whole balance would be refused for lack of
cash the command itself is freeing. Handled by excluding the original from the
reservation sum, and tested at exactly 100% committed.

**Account-side rejections live in the engine, broker-side in the broker.** The
engine knows cash and positions; the broker does not. Checking before the broker
sees the order also means the reject reason is the true one.

**The gate is passed.** Every flow the brief lists under *Testing* is covered
with no server, no database, no clock and no sleeping: submission and
acknowledgement, partial and complete fills, cancellation and replacement,
position and P&L updates. 66 tests across four crates. If the schedule broke
here, this would still be a defensible submission.

**A clippy finding worth recording:** an integration test in `tests/` is its own
crate, so a lib's `cfg_attr(test, allow(...))` does not reach it and the
workspace `unwrap_used` deny applies at full strength. Lifted explicitly at the
top of the test file rather than by weakening the workspace lint.

## 2026-08-29 — Stage B2 landed

**The worked example in `ranking.md` §6 is now a test.** If the doc and the code
disagree, the test fails — which is the reason for writing an example with
numbers in it rather than prose.

**A self-inconsistency in that example was found while writing the test and
fixed.** Participant C was described as "having held a position on day 1" while
the table gave C a day-1 turnover of **0** — so C could not have been active,
and the day-2 narrative did not follow. Corrected: C buys once on day 1 into a
name whose mark does not move, and holds through day 2, making C active on both
days. The rule the example is there to illustrate is unchanged; the example now
actually illustrates it. Caught only because the numbers had to be executed.

**Returns are `Decimal`, never `f64`.** Two participants whose returns differ in
the fifteenth place must compare deterministically, and binary floating point
cannot promise that. Rounded to a fixed `RETURN_SCALE` of 10 places so the
figure that is ranked is the same one that is reported, and so two runs cannot
disagree in the last place.

**The geometric argument is a single test:** −50% then +50% compounds to −25%,
and the participant who did nothing ranks above it. An additive ladder would
have reported that participant as flat.

**Determinism is asserted, not claimed:** three different input orderings of a
fully-tied day produce byte-identical leaderboards, because the sort ends in
`participant_id` and no two participants can compare equal.

**Ineligible participants sort last and take no placement number**, so placement
numbers stay contiguous among those who actually competed. The test pins the
uncomfortable case directly: a never-traded account's 0% beats an active
participant's −5% on the day, and it still does not place.

## 2026-08-29 — Stage B1 landed

**Serde stays out of the domain, and the cost was paid rather than avoided.**
`crates/store/src/wire.rs` is 360 lines of explicit mirror types and `From`
impls. What it buys: a rename inside `domain` cannot silently change the format
of data already on disk, and the storage format is a decision made in one place
instead of a by-product of a struct definition. Every field crosses as its raw
scaled integer — a JSON number for money would be an IEEE double, and this file
is the boundary where that mistake would be permanent.

**Every event variant is round-tripped in one test.** A new event type added
without a mapping fails there rather than at 3am against a log that will not
load. Asserted with exact equality, so a sub-cent price and a capitalised fee
have to come back bit for bit.

**Appends are idempotent, by primary key rather than by convention.** `seq` is
the primary key and a conflicting insert does nothing, so re-appending after a
crash mid-write duplicates nothing. Tested against both implementations,
including a partial re-append of the first three entries.

**One transaction per batch:** a command's events land together or not at all. A
partially-written command would replay into a state the engine never held.

**`InMemoryLog` is a fake, not a mock** — it implements the real behaviour
including idempotency, so tests written against it are testing behaviour and
survive a refactor of the SQLite side (`rust.md`).

**The replay guarantee now runs end-to-end through SQLite**, still with a
different broker seed, and the sub-cent basis (400.2000) is asserted after the
round trip through JSON.

## 2026-08-29 — day close on the engine

**A `DayClosed` event stores the day's *facts*, not the computed board.**
Closing value, prior close, turnover and activity are what happened and cannot
change; the leaderboard is recomputed from them on demand. This keeps the event
small and means a stored board can never drift from the facts behind it. The
trade, stated plainly: changing the ranking rules would change historical
boards, so in production that needs a migration. Listed in the README's
production delta.

**Closing is idempotent at the command level** — a second close of the same day
produces *no events at all*, so the sequence number does not even move. Tested
by moving the mark to 50 after a close and asserting the published board is
byte-identical and only the mark update was journalled.

**Failing closed is now load-bearing rather than theoretical.** A day cannot be
closed while any held symbol lacks a mark: `total_value` propagates the error up
through `day_entries`, so the close is refused rather than publishing a book
valued at zero.

**Active is `fills today > 0 || holds a position now`.** Both halves of "traded,
or was exposed" are covered without extra tracking: a participant who held at
open and sold out has fills > 0, and one who held through has a position.

**Turnover is gross notional on both sides**, reset by the close. It measures
how much trading was done, not what it netted to — which is the point of using
it as the tiebreak.

**A test caught a wrong assumption of mine, and the system was right.** The
turnover test assumed `rows[0]` was the participant who traded. On that day she
sold *at the mark*, so her return was 0.00% — identical to the participant who
did nothing — and the **turnover tiebreak correctly put the non-trader first**.
The same result reached with less trading is the better result, which is exactly
what `ranking.md` §2 says it should do. The assertion was fixed; the ranking was
not.

**`scripts/check.sh` added** after a commit went out with a clippy failure: the
`| head` in the verification pipeline swallowed the exit code, and `cargo build`
had reported the problem as a warning the grep skipped. The script uses
`set -euo pipefail` and no pipes, so it cannot pass by accident. The commit was
amended rather than fixed forward, because `delivery.md` says every commit is
green and a broken one in history contradicts the file that says so.

## 2026-08-29 — Stage C1 landed

**The single writer is one mutex, not a channel-and-actor.** `design.md` §12
proposed an actor; the mutex delivers the same property — a single total order
over mutations — with a fraction of the machinery, and a competition's order
volume is nowhere near one core. Building the actor anyway would be exactly the
speculative complexity `principles.md` §7 declines. The design doc has been
corrected to match the code, and the actor is listed as a production delta.

**Events are durable before they are visible.** `Engine::execute` was split into
`plan` (decide, journal, do **not** apply) and `commit` (apply), so `App` can
write ahead: plan → persist → apply. Applying first would leave state the log
cannot reproduce if the process died in between — the exact divergence the event
log exists to prevent. A plan that is never committed leaves a gap in `seq`,
which is harmless: the sequence is a total order, not a count.

**Order ids are minted server-side and resumed from the log.** The client order
id is *ours* (FIX `ClOrdID`) and a client cannot guarantee uniqueness. On
startup the minter resumes past the highest id in the log, so a restart cannot
re-issue an id that is already resting — tested by restarting against the same
file and asserting the next id is higher.

**Errors are RFC 7807 with a stable `type`, and the useful ones carry numbers.**
An illegal transition returns `409` **naming the current state** — "it is
already FILLED" is actionable where "that failed" is not. Insufficient cash
returns what was needed and what was free. Both asserted.

**Fail closed means never a wrong number, not never a response.** A portfolio
read with a missing mark returns `200` with `totalValue: null` and a
`valuationError` naming the symbol, while *closing a day* in the same state
returns `409`. Valuing a book at zero is the failure worth preventing;
refusing to answer at all is not.

**Strict parsing survives to the edge.** `10.123456`, `aapl`, `BUY`, and half a
manual execution (`qty` without `px`) are all `400`. The wire format carries
money as **decimal strings**, never JSON numbers, and requests use
`deny_unknown_fields` so a typo'd field is an error rather than a silent default.

**Verified against the real binary, not only the router.** The release build was
started on a port and driven with curl through the whole flow: the seeded broker
filled a 100-share order as 69 → 30 → 1, the portfolio valued to 100200.0000,
the day closed, and the ladder listed the participant who never traded as
`rank: null, eligible: false`.

## 2026-08-29 — Stage C2 landed, and two docs corrected against the code

**`ptc-demo` drives the same application layer the HTTP API drives**, not the
API itself: no server, no ports, nothing left running. Every timestamp is
supplied and the broker is seeded, so **two runs are byte-identical** —
verified by diffing them, not by asserting it.

**A real display bug, found by checking the arithmetic independently.**
`{:.4}` on a `rust_decimal::Decimal` **truncates rather than rounds**: a
cumulative return of 0.14169% printed as `0.1416%`, wrong in the digit a reader
checks. Fixed by rounding explicitly before formatting. Every money figure in
the demo was re-derived by hand and matches exactly; only the percentage
display was wrong.

**The dependency table in `principles.md` was wrong and is corrected.** It had
`engine` depending on `store` — impossible, because `store` persists `engine`'s
event type and the reverse would be a cycle. The application assembly that needs
both (`App`) therefore lives in `api`, and `ptc-demo` is a second binary in that
crate rather than a crate of its own. The stricter shape — `EventLog` defined as
a port *in* `engine` and implemented by `store`, letting `App` move down a layer
— is the right answer and is now a production delta rather than a silent
inconsistency.

**`design.md` §12 was corrected to say mutex, not actor**, matching what was
built and why. The doc was the bug, per the maintenance rule; the actor is
recorded in §16 as the production change.

## 2026-08-29 — README written

Written last, from the code and this log, so it describes what exists rather
than what was planned (`delivery.md`). Covers the five things the brief requires
— architecture, key design decisions, P&L and ranking approach, assumptions and
limitations, production changes — plus running instructions and the demo.

**Every number in it was checked rather than recalled.** The test count (98) is
from the suite; the ladder output quoted is diffed against an actual run; the
money figures were re-derived by hand. One claim was corrected while
proof-reading: the README asserted a Rust MSRV of 1.90 that was never tested, so
it now says what was actually used.

**The limitations section states omissions as decisions**, and the process
section lists three places the implementation proved the design wrong — the cents
money scale, the actor concurrency model, and the dependency cycle. Showing
where a design was corrected is more useful to a reviewer than implying it was
right first time.

## 2026-08-29 — Stage D1: the cockpit

Built **after** the scored work, per `delivery.md`'s cut order, and the deletion
test still holds: nothing in `crates/` refers to `ui/`, so the service is
complete with the directory removed.

**A real bug, found by comparing the rendered page against the API.** The UI
showed bob's cumulative return as `-0.0505%` where the CLI and the API both say
`-0.0506%`. Cause: `Number(d) * 100` then `.toFixed(4)` — the binary double for
`-0.0005055` lands just below the midpoint, so JS rounds down where
`rust_decimal`'s `round_dp` rounds half away from zero. **The float mistake the
entire backend is built to avoid, reappearing in the one layer that displays
it.** Rewritten to shift the decimal point by moving digits and round with
`BigInt`; checked against nine cases including the one that failed, and
re-verified in the live browser.

The lesson generalises past this repo: keeping money off floats in the service
buys nothing if the client parses it back into one to render it. Money and
returns now stay strings end to end and are formatted, never arithmetic'd.

**No CORS layer.** The Vite dev server proxies `/api` to the Rust process, so
the browser sees one origin. One less dependency in the part of the system being
scored.

**Nothing is derived client-side.** The store polls and displays; the ranking
rules shown come from the API payload (`rankedBy`, `tiebreaks`, `eligibility`)
rather than being restated in the frontend, so the two cannot drift. Same rule
as the backend: one owner per fact.

**Polling, not websockets.** The backend has no push channel and adding one is
scope the brief did not ask for; competition state changes at human speed.

**Verified running, not just building.** The API was seeded with a two-day
competition and the cockpit read back every figure — portfolios, the
cancel-after-partial keeping its 40, the replace chain `#4 ← #3`, both
leaderboards, and carol listed `never traded` with no rank.

## 2026-08-29 — audit against the brief, and two real gaps closed

Asked whether everything the brief wants is built, the honest answer needed
checking rather than recalling. Every requirement was walked line by line. Two
gaps were found.

**`REJECTED` was unreachable through the running service.** The state is
implemented, unit-tested and exercised in the full 6 × 4 matrix — but the server
constructed the broker with `Limits::default()`, which has no symbol allowlist
and no size cap, so **the broker could never actually reject anything.** A
reviewer poking the API could exercise five of the six states the brief names.

Supporting a state in the type system is not the same as supporting it in the
product. Fixed by exposing the limits as `PTC_SYMBOLS` and `PTC_MAX_QTY`, and
verified against the running binary: an off-allowlist symbol and an oversized
order both come back `REJECTED`, an allowed order comes back `ACKNOWLEDGED`.

**The README was 414 lines; the brief asks for a "short README" that "briefly
explains".** Writing a comprehensive one where an explicit instruction said
*short* is the same failure mode as gold-plating, and `challenge.md` records that
scope discipline is itself scored. Cut to 247 lines: all five required sections
present and brief, with the depth moved to `docs/design.md` and
`docs/ranking.md` where it belongs and linked from the top. Nothing was lost —
it moved to the layer that is allowed to be long.

**A third, smaller find:** making the limits configurable tripped the workspace
`expect_used` deny, because the config parsing used `expect`. That lint doing
its job on the first careless line written after it was added. `Config::from_env`
is now fallible: a mistyped `PTC_MAX_QTY` is a clean message and a non-zero exit,
not a panic.

**And the staleness rule caught itself.** `CLAUDE.md` still described a `cli/`
crate that was folded into `api`, and pointed at a README stage table that the
final README no longer has. Both corrected — the same failure the file warns
about, found by running its own audit.

## 2026-08-29 — review pass: three bugs, −91 lines, CI

A deliberate improvement pass. Three real defects found, each written as a
failing test first.

**The broker's slice budget was shared across every order.** `FillPolicy::Partial`
documented "at most `max_slices` executions **per order**", but the counter lived
on the broker, so working two orders at once let one spend the other's budget —
an order could complete on its first execution regardless of size. Fixed by
removing the counter entirely: each execution now takes at least `1/max_slices`
of the order's **original** quantity, so the bound falls out of the order itself.
**Stateless, so cross-order interference is not possible rather than merely
fixed**, and a field disappeared. Checked across 40 seeds × 5 slice settings.

**Days could be closed out of order.** Each day's return is measured against the
previous close and the ladder compounds in date order, so closing 08-29 and then
08-28 measured the earlier day against the later one's baseline and chained two
returns never computed against each other. Now refused, with the idempotent
re-close checked *first* so re-closing the latest day stays a no-op.

**A day with no participants journalled an event nobody could read.** `CloseDay`
committed a `DayClosed` carrying no entries, and the leaderboard read that
followed failed — a command that succeeded and a response that did not. Refused
in `decide`, before anything is emitted.

**Reductions, all of them things that bought nothing:**

- **`WireState` deleted (−91 lines in `store`).** Every event carrying an order
  carries it at submission, and `Order::submit` always produces `NEW` — so the
  wire format was storing a constant. Events now carry `NewOrder` **terms**, and
  the lifecycle is derived by folding later events. That is the same rule as
  positions and P&L: keep the facts, derive the rest. It also makes the
  redundancy *structurally impossible* rather than merely absent, which is why
  this was worth a refactor and not just a deletion.
- **`Qty::checked_sub_const` deleted.** It existed so `remaining()` could be
  `const`, which nothing needed, and it was a second copy of the underflow rule.
- **Four unused public functions deleted** — `TradingDay::{succ, year}`,
  `Money::{checked_neg, is_negative}`. Each was kept alive only by its own test,
  which is `delivery.md`'s "what breaks if this is deleted? nothing" case.

**Infrastructure.** GitHub Actions running the same three checks as
`scripts/check.sh`, in the same order, plus two guards that only a machine will
remember: **the demo is run twice and diffed** (if it ever differs, something is
reading a clock or an unseeded RNG), and **`domain`'s dependency tree is asserted
to contain no async runtime, HTTP, SQL or RNG** — the purity rule enforced on
every push rather than by review. The guard was checked against a dependency
that *is* present, so it is not a no-op. Plus `rust-toolchain.toml` pinning
`rustfmt` and `clippy`, so a reviewer running the checks gets the tools rather
than an error.

Net: 3,186 → 3,095 lines of non-comment source, 98 → 102 tests.

## 2026-08-29 — full audit pass: fifteen doc-vs-code corrections, two deletions

A line-by-line walkthrough of every doc against the code it describes, under the
repo's own rule that the doc is the bug. All findings were claims that were true
when written and outgrown by later work — the exact failure mode `CLAUDE.md`'s
maintenance section names.

**Counts and mechanisms:** the README said 98 tests in three places (102 since
the review pass). `design.md` §6 still described the broker's *old* slicing
("emits n partials… at or better than the limit price") — the mechanism is now
chunk-based per order, and fills are exactly at the limit, never better, which
§6 itself said two bullets later. §10's layout tree was **missing the `engine`
crate entirely** and listed a top-level `tests/` that has never existed.

**The money vocabulary:** four files still said "integer minor units (cents)"
— `code-style.md` most prominently, in its section header. The scale has been
1e4 since A0, and that correction had reached `design.md` but not the files
that cite it. All now say scaled integers, with the shared-scale reasoning.

**`principles.md`'s dependency table** claimed `domain` may use `rust_decimal`
— it never has; that dependency is `scoring`'s. The deletion test still told
readers to delete a `cli/` crate that was folded into `api` before C2 shipped.

**`rust.md` disagreed with the code it prescribes, twice, ironically:** its
`OrderState` example stored `avg_px` in the fill states — the *stored rounded
average* that `code-style.md` forbids — where the real enum accumulates `cost`.
And its `Qty` example rejected zero, where the real constructor allows it
because an unfilled order has `filled == 0`. Both examples now show the real
code, and each picked up the sentence explaining why the real shape is right.
Also corrected there: a `Clock` trait that has never existed (time crosses as a
`Timestamp` value — a value needs no port), an `anyhow` policy for a dependency
we never took, `engine` missing from the thiserror list, and a proptest claim
describing tests that were never written rather than the two that were.

**Deleted:** two Pinia getters (`symbols`, `marks`) no component ever read —
dead on arrival in the cockpit commit — and the phantom claims above.

**Verified clean in the same pass:** `forbid(unsafe_code)` in all eight crate
roots and binaries, workspace lints inherited by all six crates, zero
TODO/FIXME markers, no orphaned docs, every markdown link resolving, `check.sh`
green, the demo byte-identical across two runs, `vue-tsc` and the UI build
clean.

## 2026-08-30 — the study-report audit: nine findings from seven fresh readers

Seven independent deep-read passes over the whole repository (one per
subsystem, made while producing the interview study report) surfaced nine
issues no earlier audit had caught — each a claim or behaviour one notch out of
line with the repo's own rules.

**Behaviour fixed:** the env config was strict for `PTC_SYMBOLS`/`PTC_MAX_QTY`
but silently repaired the other three (`PTC_SEED=abc` became 42) — half the
config obeyed the documented fallible rationale and half did not; all five are
now strict via one `parse_var` helper, verified live. The money parser carried
a provably unreachable second-dot check (any second `.` already fails the
all-digits test) — deleted, per the no-impossible-scenarios rule that governs
non-accounting paths.

**Docs corrected to match code:** `design.md` §6 listed *insufficient cash*
among broker reject triggers (the broker cannot see a book; cash is an engine
pre-trade refusal); §7's sell maths was written in the avg-form the code's own
comments decline (now division-free, matching `cost_removed`); the "CI asserts
that tree" phrasing (it is a denylist guard); `rust.md`'s transition-signature
sketch was missing the `ordered: Qty` parameter; `build-order.md` A4 still said
`EventLog` is defined in the engine (it lives in `store`; that is why `App` is
in `api`). `error.rs`'s comment overstated the Overflow→500 mapping (parse-time
overflow is 400; only ledger overflow reaches the catch-all). The `update_marks`
comment now states its atomicity boundary precisely (parse-atomic; past that,
each mark is its own transaction — fine for marks, never for fills).
`daily_results`' "order-independent" doc line now says what is actually
invariant (the ranked output, not row order).

**Documented for the first time:** why `ChaCha8Rng` and not `StdRng` — `StdRng`
may change algorithm between `rand` releases, which would silently break
same-seed replay on an upgrade; ChaCha8's stream is pinned. The argument
existed only in a head; now it is in the broker's crate docs.


## 2026-08-30 — fees surfaced; lot methodology decided

**Fees are now reported, not only booked.** The record always kept price and
fee separate (as FIX does — Commission tag 12, MiscFees 136–139); what was
missing was visibility: the portfolio now folds `fees_paid` from the same
`OrderFilled` events and reports it through the API, the cockpit and the demo.
The demo's bob makes the case by himself: unrealized −0.5500, fees 0.5500 — a
flat stock, and the whole loss is the commission, now legible.

**One lot view, on purpose.** Average cost stays the only view in the engine:
it is the brief's own term, it is what every blotter shows, and the competition
scores closing value — which is invariant to lot methodology, since any method
moves basis between realized and unrealized without changing their sum
(worked in design.md §7: +150/avg vs +200/FIFO, offset exactly by the basis
left behind). The books/tax view — FIFO default, specific-identification in
practice — is declined as a build and documented as a derivation: every fill
is already in the event log, so a lot ledger is a pure read-side fold. One log,
many views; building the second view for a split nothing scores would be the
gold-plating the brief warns against.

## 2026-08-30 — per-order fees, and where orders actually live

**Per-order fees are an engine projection, not a lifecycle field.** A1's
decision stands — `OrderState` tracks what executed, gross — so the engine
accrues `order_fees` in `apply(OrderFilled)`, exactly as it accrues
`day_turnover`: same events, same single writer, one owner per fact. `fee_of`
exposes it; every order surface (API, cockpit column, demo fill lines, the
replay snapshot) now reports fees beside cost, never inside it.

**The `From<&Order>` DTO impl was deleted, deliberately.** `order_view(order,
fees)` takes the fee as an explicit argument so no call site can forget it and
silently render zero — the compiler walked every route to the new signature.
Replacements start at zero: fees stay with the order that incurred them.

**Also recorded, because the question was asked: orders are not stored.** The
database has one table — `events(seq, at, kind, payload)` — and orders are a
fold over it, held in the engine's `BTreeMap` and rebuilt by replay. Which is
why this feature, like `fees_paid` before it, materialised retroactively:
restarting the new binary on the running cockpit's log back-filled per-order
fees for fills booked before the projection existed. The fee was in every
`order_filled` row all along; the projection just started reading it.

## 2026-08-30 — the contract, served: /openapi.json + /docs

**A hand-written OpenAPI 3 contract, served by the API itself,** with Swagger
UI at `/docs`. Hand-written rather than derived (utoipa was the alternative)
because the spec here is a *decision document*: the descriptions carry the
semantics no derive macro knows — the idempotent close, fail-closed valuation,
strict parsing, the two execution modes, the numbers errors carry. The drift
risk of a hand-written spec is closed the same way everything else here is:
**a test** — the spec's path+method set is asserted equal to the router's, so
the two cannot disagree silently.

**The operations are tagged with the brief's seven interface bullets
verbatim**, so opening /docs shows the requirement list as the table of
contents — the spec reads as the brief, satisfied.

The docs *page* loads Swagger's assets from a CDN and degrades to the raw
contract when offline; the API itself gains no dependency. Verified live:
Try-it-out executed GET /ladder from the browser and rendered alice ranked
and carol `eligible: false`.

## 2026-08-30 — reference data: found by using our own walkthrough

**The gap:** the first person to actually run the test card (the operator, not
the author) asked how a client learns which symbols are tradable — and the
answer was "by being rejected." The allowlist lived in server config and the
startup log; the API never exposed it. Same one layer down: marks could be
POSTed but not read back. Discovery by rejection is not discovery.

**The fix, shaped the institutional way:** reference data is the venue's to
answer — FIX calls it a SecurityList — so `limits()` joined the `Broker` port,
and `GET /instruments` returns the allowlist (`symbols: null` = unrestricted)
plus any size cap. `GET /market/prices` returns current marks in symbol order,
the read counterpart of the existing write. Both are in the OpenAPI contract
under the brief bullets they serve, the drift-guard test learned the two new
routes, and the cockpit now uses them: the submit form shows the tradable
universe as chips, and the mark form shows what the market currently says.

Tested both ways: unrestricted broker answers `symbols: null`; a restricted one
lists exactly its universe and cap; marks read back byte-identical to what was
posted, sorted. The walkthrough server was upgraded mid-walkthrough and the
operator's three participants survived the restart by replay — as designed.

## 2026-08-30 — the security master: instruments become first-class, event-sourced

**The ask:** full CRUD on instruments, with the spec the earlier tick-size
conversation identified — minimum price increment, lot step, executable size
limit. **The architectural consequence, embraced rather than dodged:** editable
reference data is *state*, and state here means events. `InstrumentUpserted` /
`InstrumentRemoved` join the log; the registry is a fold; replay rebuilds it;
the snapshot parity test now covers it. `PTC_SYMBOLS` stopped being broker
config and became a **seed** — applied once, as journalled events, only into an
empty registry, so a restart never fights what the API has since edited.

**Violations are venue-style rejections, not refused commands.** An off-list,
off-tick, off-lot or over-cap submission produces a *recorded `REJECTED` order*
carrying `UnknownSymbol`, `PriceOffTick`, `QtyOffLot` or `ExceedsSizeLimit` —
exactly as an exchange answers it, and exactly the Reg NMS behaviour discussed
for stocks: verified live by setting AAPL to a penny tick over PUT and watching
a $10.0050 limit come back REJECTED. Replacements face the same gate. Cash and
position stay engine refusals: the venue cannot see a book.

**The broker slimmed accordingly** — this morning's `limits()` on the Broker
port lasted half a day and is superseded: reference data belongs to the engine's
master, so the mock now only acks and slices. Two broker tests moved to the
engine where the behaviour now lives. Delisting is guarded: refused with counts
while any working order or position references the symbol — a venue must not
strand what it cannot unwind. Spec changes do not retroactively re-judge working
orders: the venue changed its rules; it did not unwind your order.

**The drift guard earned its keep in real time:** four routes were added and
the OpenAPI test failed the build until the contract documented them — which is
precisely the failure mode it was written to catch, demonstrated same-day. And
one honest limitation named in the README: a tick here is one value per symbol,
not a function of price, so the *conditional* regulatory tick (penny above $1)
remains unmodelled.

103 tests; cockpit gains an instruments admin block and per-symbol tick chips.

## 2026-08-30 — POST /reset: the world is disposable; history within it is not

**The design question a reset poses to an event-sourced system:** append a
`CompetitionReset` event and teach every fold to zero itself, or restore the
boot world? The first preserves dead history that taxes every boot and has no
consumer. Taken instead: **reset ≡ delete-the-file-and-reboot, self-served** —
`EventLog::clear()` (the log is append-only *within* a competition; the
competition itself is the operator's to discard), a reborn engine, and —
the subtle requirement — **a reborn broker**: reusing the old one would leak
the previous world's RNG stream and id counter into the new one, so `App` now
holds a broker *factory* rather than a broker, and a reset world is exactly as
deterministic as a booted one. Verified: after reset, order ids and broker ids
both mint from 1 again.

`PTC_SYMBOLS` seeding moved from `main` into `App::apply_seeds`, applied on
open *and* on reset, still only ever into an empty registry. The drift guard
fired again — `/reset` undocumented failed the build until the contract
carried it — and the cockpit gained a guarded reset button in the top bar.
Beyond the brief, labelled as such in the contract: this is the paper system's
demo-account reset, a feature real brokers ship.

## 2026-08-30 — the third id: every fill gets its tid

**Raised in review, correctly:** the trio we cite from FIX and from sigma —
cloid / oid / tid — was two-thirds implemented. `cloid` mints at submit and
`oid` at the ack, but executions carried no identity at all: the trade
blotter's rows were anonymous facts. In a real system the `ExecID` (FIX 17) is
what fills are deduped and disputed by; a fill you cannot name is a fill you
cannot reconcile.

**Now:** `ExecutionId` in the domain; the broker mints one per execution from a
single per-world counter — including **explicit, operator-dictated executions**,
because they are still *booked at the venue* and the venue numbers its own
tape. `OrderFilled` events carry it, the wire persists it, and a per-order
trade blotter is queryable at `GET /orders/{id}/executions` — the engine's
`order_executions` projection, rebuilt by replay like everything else. The demo
narrates it (`fill #1 tid 1 → …`).

The confirmation timeline, now complete and stated: **cloid at submit · oid at
ack · tid per fill.** And the honest production note rides in the contract: the
tid is the dedup key for redelivered reports; here every command mints fresh,
because the operator *is* the venue.

105 tests; the drift guard forced the new route into the contract before the
build passed, as usual.

## 2026-08-30 — nothing on the wire is called "orderId"

**Raised in review, and FIX-backwards until fixed:** the API said `orderId` in
three places (the Execute body, the OrderView's own id, the blotter response),
and in every one it meant the **client** order id — while in FIX, *OrderID* is
tag 37, the **broker's** id. A reviewer fluent in the protocol would read our
field exactly wrong, and id ambiguity is how fills get booked against the wrong
order in real life.

The wire now speaks the trio in full and nothing else: `clientOrderId`
(ClOrdID 11), `brokerOrderId` (OrderID 37), `execId` (ExecID 17) — in every
request body, every response, and even the path-parameter names
(`/orders/{clientOrderId}`), with the OrderView schema carrying the rule as its
own description. The internal domain never had the problem (`ClientOrderId` /
`BrokerOrderId` / `ExecutionId` were always explicit); the edge was the only
place the shorthand had crept in — which is the serde-boundary argument paying
out once more: one wire layer to fix, zero domain commits.

## 2026-08-30 — brokerOrderId never disappears from the view

**Raised by the operator mid-walkthrough:** should `GET /orders/{clientOrderId}`
contain the `brokerOrderId`? It did — until the order turned terminal, when it
went null, because the *state machine* drops the venue id on terminal states (a
live-correlation concern, correctly). Wrong rule to leak into the API: post-
trade is exactly when you quote the broker's id back at the broker, and a
blotter that forgets the counterpart reference at completion fails at the
moment reconciliation starts.

Fixed as an engine projection (`order_broker_ids`, recorded at the ack), so the
domain enum stays exactly as A1 decided and the *view* keeps the id forever.
The new rule is crisp and in the contract: **null only ever means the venue
never assigned one** — pre-ack `NEW`, or `REJECTED`. `order_view` takes it as a
third explicit argument, same reasoning as fees: no call site can forget it and
silently render null. The old test asserting terminal-drops-it now asserts the
opposite, with the reconciliation argument as its message.

## 2026-08-30 — the tape on the Simulation page, keyed by ExecID

**Operator ask:** the Simulation page should show all orders by cloid, and
executions "by brokerOrderId or executionId — whichever is better."

**ExecID is better, and the reason is definitional:** the brokerOrderId names
the *order* at the venue — every fill of one order shares it — while the ExecID
names *each fill*, and a blotter is a list of fills. So `GET /executions` (the
tape: every execution in the world, ExecID order, which is mint order, which is
chronological) keys rows by tid and carries cloid and oid as correlating
columns — the complete id trio on every row, which is itself the teaching aid.

The Simulation page's live column now stacks Portfolios → Orders (by cloid,
with a new oid column on the shared blotter) → Executions (the tape) →
Rankings, so a run narrates on the left while the trio materialises on the
right. One new read endpoint; the drift guard demanded its documentation
before the build passed, as always.

## 2026-08-30 — the Journal: the log, read back with words on

**Operator ask:** the Console should match the Simulation page's panels — and
acting through Swagger should be *watchable* as logs.

The second ask has the shortest possible answer in this architecture: **the
system already is a log; it just wasn't exposed as one.** `GET /events` reads
the journal itself — no second stream, no logging framework — rendering each
event as a human sentence ("tid 1: fill 40 AAPL @ 10.0000 on cloid 1 (alice),
fee 0.2000"), with an `?after=<seq>` cursor so the cockpit polls incrementally
rather than refetching the world's story every two seconds. Fill enrichment
(symbol, participant) joins through the engine's own projections at read time.

The Console's left column gains the Journal panel (newest first, seq + clock +
sentence) beside the controls, and the right column gains the tape — so both
pages now show the same five surfaces, and anything done in Swagger, curl or
the cockpit itself narrates in the Journal within a poll. Reset clears the
cursor. The drift guard demanded the contract entry before the build passed —
sixth catch.
