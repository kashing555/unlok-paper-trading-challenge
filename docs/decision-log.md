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
