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
| `domain` | *nothing* (std + `rust_decimal` only) | async, HTTP, SQL, time, RNG, config |
| `scoring` | `domain` | async, HTTP, SQL, time, RNG |
| `broker` | `domain` | HTTP, SQL — RNG only via an injected seed |
| `store` | `domain` | HTTP, `scoring`, `broker` |
| `engine` | `domain`, `broker`, `store`, `scoring` | HTTP, CLI, UI |
| `api` | `engine`, `domain` | *is* the outside; everything may know it exists, it may know nothing back |
| `cli` | `engine`, `domain` | `api` |
| `ui` | `api` over HTTP only | everything else |

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

## Part 3 — Detecting coupling

Coupling is not a feeling. Each of these is a check that either passes or fails:

- **The deletion test.** Delete `ui/`, then `api/`, then `cli/`. Does everything
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

### Working on different things at once

The seam is the contract, and the contract is a **type agreed before either side
is implemented**. Define the events, the command enum and the port traits first;
then both sides can be built against them independently and meet at a compile
check rather than at a merge conflict.

One owner per crate at a time. Cross-crate changes get their type change landed
first, alone, so the other side can move.

## Part 4 — SOLID, kept honest

Cited because reviewers ask. Applied selectively, because applying all five
uniformly to a two-day exercise is cargo cult:

| | Verdict here |
|---|---|
| **S**ingle responsibility | **Applied hard.** One reason to change per module — the explanation test above is SRP with a stopwatch |
| **D**ependency inversion | **Applied at real seams only** — `store` and `broker` are ports the engine depends on abstractly. Not applied to `domain` types, which are concrete because they have exactly one implementation |
| **I**nterface segregation | **Applied.** Small ports (`EventLog`, `Broker`) over one god-trait |
| **O**pen/closed | **Mostly declined.** Extension points built for imagined future variation are `baseline.md` §2's "speculative flexibility" wearing a principle's name. The one place it earns itself: the ranking strategy, because the brief explicitly asks us to consider alternatives, so more than one really exists |
| **L**iskov substitution | **Not a live concern** — no inheritance hierarchies |

Also declined, deliberately: a DI framework · a generic `Repository<T>` over a
single store · an interface per struct · a mapper layer between identical
structs. Each adds indirection and buys nothing at this size.
[YAGNI](https://martinfowler.com/bliki/Yagni.html) is not a licence to make a
mess; it is a reason not to build the second thing until there is a second thing.

## Part 5 — Trading-system principles

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

**Money is exact.** Integer minor units, rationals for averages, decimal strings
on the wire. No float touches a P&L path. See `code-style.md`.

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

## Part 6 — Build the engine first

Detail in [`docs/build-order.md`](../docs/build-order.md). The principle:

**The engine is the product; everything else is a way to reach it.** Order
lifecycle, position and P&L accounting, and the broker are what the brief
scores. HTTP, CLI, persistence and the cockpit are transport and can be added
without changing a line of the core — provided the dependency rule above held.

So the core is built and fully tested standing alone, before any transport
exists. If that ordering feels awkward at any point, the dependency rule has
already been broken somewhere and this is the early warning.
