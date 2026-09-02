# Engineering principles

Extends `baseline.md` with the structural rules — what depends on what, and why
nothing is allowed to couple. `baseline.md` governs *how much* code to write;
this file governs *where it goes*.

Named principles are cited because they are the vocabulary the reviewer already
has. Using the real name of a pattern is cheaper than describing it, and being
able to say which parts of it we rejected is the actual signal.

---

## Part 1 — The rule everything else follows from

**One owner per fact.**

Every piece of state has exactly one component authorised to change it. Position
is owned by the event fold, not by the fill handler *and* a cached row. Order
state is owned by the transition table, not by whichever handler last touched
it.

Two copies of one fact are permitted to disagree, and given time they will. Most
of the bugs worth naming in an order management system are this bug: an inferred
position reconciled against a venue, a cached average re-derived on a different
path, a status field updated in two places.

Everything below is a mechanism for keeping this true under change.

## Part 2 — Separation of concerns

### The dependency rule: dependencies point inward, always

Concentric layers, from **ports and adapters** ([Cockburn](https://scalastic.io/en/hexagonal-architecture/)).
The core defines what it needs; the outside implements it. Nothing in the core
knows the outside exists.

| Crate | May depend on | Must never know about |
|---|---|---|
| `domain` | *nothing of ours* — std, `chrono` (clock feature off), `thiserror` | async, HTTP, SQL, clock, RNG, config |
| `scoring` | `domain`, plus `rust_decimal` for returns | async, HTTP, SQL, clock, RNG |
| `broker` | `domain` | HTTP, SQL — RNG only via an injected seed |
| `engine` | `domain`, `broker`, `scoring` | HTTP, SQL, CLI, UI |
| `store` | `domain`, `engine` (for the event type it persists) | HTTP, `scoring`, `broker` |
| `api` | everything above | *is* the outside; everything may know it exists, it may know nothing back |
| `ui` | `api` over HTTP only | everything else |

**Corrected against the implementation (2026-08-29):** this table first had
`engine` depending on `store`, which is impossible — `store` persists
`engine`'s event type, so it depends on `engine`, and the reverse would be a
cycle. The application assembly that needs both (`App`) therefore lives in
`api` alongside the HTTP surface, and the demo CLI is a `demo` subcommand of
that crate's single binary rather than a crate of its own. The stricter version — the `EventLog`
port defined in `engine` and implemented by `store`, letting `App` move down a
layer — is the right shape and is noted as a production delta.

An arrow pointing the wrong way is a design error, not a style preference. In a
Cargo workspace it is also a **compile error** — a crate cannot import what is
not in its `Cargo.toml`. That is the reason for crates rather than modules in
one binary: the boundary is enforced by the toolchain instead of by discipline,
and discipline is what erodes at 2am on day two.

### Functional core, imperative shell

[Bernhardt's](https://www.destroyallsoftware.com/screencasts/catalog/functional-core-imperative-shell)
formulation, one level below ports and adapters: the core is not merely isolated
from infrastructure, it is **pure**. No async, no I/O, no clock, no randomness.

- **Core** (`domain`, `scoring`) — decisions. Given a state and an input, return
  the next state and the events. Total functions where possible; errors as
  values, not exceptions.
- **Shell** (`engine`, `api`, `store`) — effects. Reads, writes, awaits, and
  calls the core to decide.

The practical test: **a rule that needs a running server to test is in the wrong
layer.** Time and randomness are injected as values (`now: Timestamp`, `rng:
&mut impl Rng`), never read ambiently. Ambient `Utc::now()` inside a decision
makes it untestable and non-reproducible in one stroke.

### Commands in, events out

The core never mutates anything reachable from outside itself. It accepts a
**command** (an intent that may be rejected) and returns **events** (facts that
already happened, and cannot be). The shell persists the events and applies them
to projections.

This is what makes replay work, and replay is what proves there is no hidden
state. It also gives every rejection a place to live: a command can fail
validation and produce no events, which is a normal outcome rather than an
exception path.

## Part 3 — Type-driven design

Two principles that turn a class of runtime bug into a compile error. The
mechanisms are in [`rust.md`](rust.md); the reasoning is here.

**Make illegal states unrepresentable** ([Minsky](https://blog.janestreet.com/effective-ml-revisited/)).
A struct with `filled_qty`, `cancelled_at` and `reject_reason` as optional
fields admits "cancelled AND rejected, with a fill" — a state no order can be
in, which some handler will eventually construct and no test will cover. An enum
whose variants carry exactly their own data cannot express it. Prefer the shape
where the bad state has no spelling.

**Parse, don't validate** ([King](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)).
Validation checks a value and moves on, leaving it illegal-but-checked and the
check re-runnable downstream — which is where "we validated it three layers up,
mostly" comes from. Parsing returns a *type that carries the proof*, so every
function taking a `Qty` is relieved of re-checking it. Constructors are fallible
and fields are private; an invalid value never exists rather than existing and
being caught.

Both are the same move: **push the guarantee into the type so the compiler
maintains it**, rather than into a convention that reviewers maintain. That is
the same trade as enforcing the dependency rule with crates in §2.

Where the guarantee is *not* available — order state is loaded from an event log
at runtime, so its type is not known statically — we say so and take the next
best thing (an exhaustive `match` with no wildcard arm). `rust.md` has that
decision in full. Knowing which guarantee you can afford is the skill; reaching
for the strongest one everywhere is how a codebase becomes unreadable.

## Part 4 — Detecting coupling

Coupling is not a feeling. Each of these is a check that either passes or fails:

- **The deletion test.** Delete `ui/`, then `api/` — the demo CLI is a
  subcommand of that crate's binary, so it goes with it. Does everything
  the brief scores still compile and test? If not, scored logic has leaked into
  transport. *(This is why the README can promise the service is complete with
  `ui/` deleted — it is a property we can actually run.)*
- **The swap test.** Replace the SQLite store with an in-memory one. Does any
  file in `domain` or `scoring` change? If yes, the abstraction leaked.
- **The purity test.** Does `domain/Cargo.toml` list any async runtime, HTTP,
  SQL, clock or RNG dependency? If yes, the core is not a core.
- **The parallel-work test.** Can two people edit `scoring` and `api` at the same
  time without conflicting? If not, the seam between them is not a real seam.
- **The explanation test.** Can one file's job be stated in one sentence without
  the word "and"? Two "and"s is two files.

Run these as questions during review, not as an afterthought.

### Naming the coupling: connascence

"Coupling is bad" is not actionable. [Connascence](https://en.wikipedia.org/wiki/Connascence)
(Page-Jones) is the precise version: two components are connascent when changing
one requires changing the other. It grades on three axes — **strength** (how hard
the change is), **locality** (how far apart they sit), and **degree** (how many
things are affected).

The rule that makes it useful: **the further apart two things are, the weaker
their connascence must be.** Strong connascence inside one function is fine —
that is just code. The same connascence across a crate boundary is a design
error, because nobody reading one side can see the other.

Static forms, weakest to strongest — the compiler can see all of these:

| Form | Two things agree on | Across our crate boundaries |
|---|---|---|
| **Name** | an identifier | fine — this is what an API is |
| **Type** | a type | fine, and the target to convert others into |
| **Meaning** | what a bare value *signifies* | **not allowed** — this is `i64`-means-cents |
| **Position** | argument order | avoid — a 4-arg call is a swap waiting to happen |
| **Algorithm** | a shared procedure | **not allowed** — put it in `domain` and call it |

Dynamic forms — execution order, timing, value, identity — are **strictly worse
because the compiler cannot see them at all**, and they are what our structure
exists to eliminate:

- **Connascence of value** across components *is* the two-copies-of-one-fact bug
  from §1: a cached position and a folded position must change together, and
  nothing enforces it. The event log removes it by deleting one of the copies.
- **Connascence of execution order** is what the single-writer loop removes: with
  one writer there is one order, and it is the log's.
- **Connascence of timing** is what injected clocks remove.

The practical move is almost always **convert connascence of meaning into
connascence of type**. A function taking `(i64, i64)` where the first is cents
and the second is a scaled price shares meaning with every caller and cannot be
checked; taking `(Money, Px)` shares only type, and the compiler checks it. That
one conversion is most of what the newtypes in §3 are for.

### Working on different things at once

The seam is the contract, and the contract is a **type agreed before either side
is implemented**. Define the events, the command enum and the port traits first;
then both sides can be built against them independently and meet at a compile
check rather than at a merge conflict.

One owner per crate at a time. Cross-crate changes get their type change landed
first, alone, so the other side can move.

## Part 5 — SOLID, kept honest

Cited because reviewers ask. Applied selectively, because applying all five
uniformly to a two-day exercise is cargo cult:

| | Verdict here |
|---|---|
| **S**ingle responsibility | **Applied hard.** One reason to change per module — the explanation test above is SRP with a stopwatch |
| **D**ependency inversion | **Applied at real seams only** — `store` and `broker` are ports the engine depends on abstractly. Not applied to `domain` types, which are concrete because they have exactly one implementation |
| **I**nterface segregation | **Applied.** Small ports (`EventLog`, `Broker`) over one god-trait |
| **O**pen/closed | **Mostly declined.** Extension points built for imagined future variation are `baseline.md` §2's "speculative flexibility" wearing a principle's name. The one place it earns itself: the ranking strategy, because the brief explicitly asks us to consider alternatives, so more than one really exists |
| **L**iskov substitution | **Not a live concern** — no inheritance hierarchies |

**CUPID** ([North](https://dannorth.net/blog/cupid-for-joyful-coding/)) is the
more useful lens for a codebase this size, and deliberately not a set of rules —
they are *properties*, things code is closer to or further from, with a direction
of travel. Two carry real weight here:

- **Predictable** — does what it appears to, deterministically, and is
  observable. In a system whose output is a ranking, this is not a nicety; it is
  the requirement the brief actually states.
- **Unix philosophy** — one thing well, composed. `domain` decides, `store`
  persists, `api` translates. The pipeline is the design.

Composable, Idiomatic and Domain-based are the reason for the crate split, the
naming conventions in [`rust.md`](rust.md), and using the words *fill*, *mark*
and *ladder* in code rather than *update*, *value* and *list*.

Also declined, deliberately: a DI framework · a generic `Repository<T>` over a
single store · an interface per struct · a mapper layer between identical
structs. Each adds indirection and buys nothing at this size.
[YAGNI](https://martinfowler.com/bliki/Yagni.html) is not a licence to make a
mess; it is a reason not to build the second thing until there is a second thing.

## Part 6 — Trading-system principles

The general rules above are true of any backend. These are the ones this domain
adds, and they are where the 20 years shows.

**Single-writer principle.** One thread owns a piece of state; all mutations
route through it. Removes contention *and*, more importantly here, gives a
single total order over events. From
[Thompson](https://mechanical-sympathy.blogspot.com/2011/09/single-writer-principle.html)
and the [LMAX architecture](https://www.martinfowler.com/articles/lmax.html),
whose Business Logic Processor runs in-memory, event-sourced and
**single-threaded** at ~6M orders/sec — proof that "single-threaded" and "slow"
are unrelated, and that the interesting constraint is usually ordering rather
than throughput.

**Determinism is a feature, and it is testable.** Same inputs, same outputs,
every run and every machine. Injected clock, seeded RNG, no `HashMap` iterated
into ordered output, total-order sorts. Replay of the event log must reproduce
state exactly — that is a test we run, not a property we assert.

**Never infer what you were told.** If the broker reports a fill, that is the
position. Deriving a second, independent belief and reconciling it is the bug
class in Part 1, and it is the most expensive one in this domain.

**Idempotency at every boundary.** Every external message carries a key
(`client_order_id`, `seq`). A redelivered execution report is a no-op. Networks
retry, and a duplicate fill is a real position and real money.

**Fail closed on ambiguity, fail fast on invariants.** Unreadable state → refuse
the action rather than guess: inaction is recoverable, a wrong fill is not. And
an invariant break in cash, position or P&L **panics or errors loudly** — it
does not log a warning and continue. This is the documented override of
`baseline.md` §2: in accounting, the impossible scenario is precisely the one
worth handling, because it is silent otherwise.

**Money is exact.** Scaled integers, a cost-and-quantity rational for averages,
decimal strings on the wire. No float touches a P&L path. See `code-style.md`.

**Time is data.** Every event carries the timestamp it happened at, supplied by
the caller. Nothing in the core reads a clock. Beyond testability, this is what
makes a competition day closable at a boundary rather than "whenever the job
happened to run".

**Mechanical sympathy** — the LMAX lineage's other half, on cache lines,
allocation and branch prediction — is **out of scope**, named so it is clear
that is a decision. A paper trading competition is not latency-bound, and
optimising for it here would be the same speculative-complexity error as OCP
above. It is the first thing to reach for if this ever became an actual matching
engine.

## Part 7 — Principles we decline

Naming these matters as much as the ones we keep — an unexamined principle
applied out of context is how systems get worse while everyone follows the rules.

**Postel's law / the robustness principle** — *"be conservative in what you
send, be liberal in what you accept."* **Rejected outright on every input path.**
Being liberal in what we accept is precisely how a malformed order becomes a
real position. An order with a missing side, an unparseable price, or a quantity
we had to guess at gets **rejected loudly at the boundary**, not repaired into
something plausible. In this domain a rejected order costs a retry; an accepted
misinterpretation costs money and is discovered later. Strict parsing, no
coercion, no defaults filled in for absent fields. (This is the same instinct as
fail-closed in §6, applied at the edge rather than the middle.)

**DRY, taken as far as it will go.** Two pieces of code that look alike but
change for different reasons are not duplication, and merging them creates a
coupling that has to be un-merged later under pressure. *Duplication is cheaper
than the wrong abstraction* ([Metz](https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction)).
DRY applies to **knowledge** — the fee convention, the tiebreak order — not to
syntax. Deduplicate the rule; leave the two similar-looking functions alone
until they demonstrate they change together.

**Speculative generality.** Configuration, plugin points, and type parameters
added for a second case that does not exist. Covered by `baseline.md` §2 and by
declining OCP in §5, restated here because it arrives disguised as good practice.

**Layers that only forward.** A mapper between two identical structs, a service
that calls one repository method, a `Repository<T>` over a single store. Each
adds a hop to read and buys nothing at this size.

**Defensive copying and null-guarding everywhere.** In a language with ownership
and no null, this is noise that hides the two or three places where a check is
load-bearing.

## Part 8 — Build the engine first

Detail in [`docs/build-order.md`](../docs/build-order.md). The principle:

**The engine is the product; everything else is a way to reach it.** Order
lifecycle, position and P&L accounting, and the broker are what the brief
scores. HTTP, CLI, persistence and the cockpit are transport and can be added
without changing a line of the core — provided the dependency rule above held.

So the core is built and fully tested standing alone, before any transport
exists. If that ordering feels awkward at any point, the dependency rule has
already been broken somewhere and this is the early warning.
