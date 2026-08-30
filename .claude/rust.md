# Structuring Rust

Extends `principles.md` — the same rules, expressed in the mechanisms this
language actually gives us. Where a principle can be enforced by the compiler
rather than by review, that is the version we want.

## Which inherited patterns survive the translation

*PoEAA* is 2002 and Rust 1.0 is 2015, so the question "does this pattern still
apply?" is a real one and gets a real answer rather than an appeal to authority.
**We take the language-neutral ideas and decline the OOP workarounds.**

**Taken, because they are about data and not about objects:**

| Pattern | Why it survives |
|---|---|
| **Money** (Fowler) | A value object with exact arithmetic. Java needs ceremony for this — final fields, hand-written `equals`/`hashCode`, defensive copies, and it *still* cannot stop `money + price` compiling. Rust makes all of it free: `derive(PartialEq, Eq)`, `Copy`, move semantics, and a newtype that costs zero bytes and **rejects `Money + Px` at compile time**. The pattern is stronger here than where it was written |
| **Event sourcing** (Fowler) | State as a fold over an append-only log. Nothing object-oriented about it — it is closer to a `fold` than to a class |
| **Single-writer / LMAX** (Thompson, Fowler) | LMAX was Java *fighting the JVM* — the Disruptor's pre-allocated ring buffer exists largely to dodge GC pauses. Rust has no GC and deterministic destruction, so the architecture is **easier** here, not ported |

**Declined, because they solve problems Rust does not have** — most already
declined in `principles.md` §7, and this is why:

| Pattern | Why it does not translate |
|---|---|
| **Repository / Unit of Work / Identity Map** | Machinery for a mutable object graph synced to rows. An append-only event log has no identity map problem to solve |
| **Active Record** | Needs a mutable object that *is* a row. Fights ownership, and hides I/O behind a field access |
| **Lazy Load** | Requires hidden mutation behind a getter — exactly what `&self` forbids |
| **Null Object / Special Case** | Workarounds for `null`. Rust has `Option` and sum types |
| **Service Layer + DI container** | Constructor injection substitutes for the absence of first-class functions and generics. Rust has both |

**The lineage we actually draw on is the ML family, not enterprise OOP.** ML
here is *Meta Language* — Robin Milner, Edinburgh, 1973 — and its descendants
Standard ML, OCaml and F#, alongside Haskell. Nothing to do with machine
learning; the acronym collision is unfortunate and worth spelling out before a
reviewer trips on it.

What Rust inherited from that tradition is most of what this design leans on:
**sum types** (a Rust `enum` is an ML datatype carrying data, not a C enum),
**exhaustive pattern matching** (why a wildcard arm in the transition match is a
review failure), **`Option`/`Result` instead of null and exceptions**,
Hindley–Milner-style **type inference**, and **immutability by default**. The
first `rustc` was itself written in OCaml, until it self-hosted in 2011.

So: newtypes (Haskell), parse-don't-validate (King, Haskell), illegal states
unrepresentable (Minsky, OCaml), exhaustive sum types, functional core — §3's
content. All of it predates Rust and fits it natively, because Rust took its
type system from the same place.

The two Fowler patterns we keep are the two that were never really about objects.

## Crate or module?

**A new crate when the boundary must be enforced**, when it needs a different
dependency set, or when it could be tested standing alone. **A module when it is
cohesion inside one boundary.**

Every row of the dependency table in `principles.md` §2 is a crate specifically
because the rule must be a **compile error** — a crate cannot import what is not
in its `Cargo.toml`. Modules inside one binary would leave the same rule as a
convention, and conventions are what erode on day two.

Do not create a crate to hold one struct. The test is whether the boundary
carries a dependency rule.

## Module layout

- 2018-style paths: `oms.rs` alongside `oms/order.rs`. No `mod.rs`.
- **`lib.rs` is the crate's public surface and nothing else** — module
  declarations and re-exports. Reading it should tell you what the crate does
  and what it exposes, in one screen.
- **Private by default.** `pub(crate)` for cross-module internals, `pub` only on
  the intentional surface. A `pub` on something that only one sibling module
  calls is a leak, and leaks are what later become "we can't change that".
- One concept per file — the explanation test from `principles.md` §4.

## Types: parse, don't validate

Constructors are fallible and fields are private, so an invalid value **cannot
exist** rather than existing-but-checked ([King](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)):

```rust
pub struct Qty(i64);                     // field private — no `Qty(-5)` from outside

impl Qty {
    pub fn new(shares: i64) -> Result<Self, DomainError> {
        if shares >= 0 {
            Ok(Self(shares))
        } else {
            Err(DomainError::NegativeQty(shares))
        }
    }
}
```

Note the invariant is `>= 0`, not `> 0`: zero is a *legal* quantity — an
unfilled order has `filled == 0` and a closed position has `qty == 0`. The
stricter "an **order** must be for more than zero shares" rule lives on the
order constructor, which is the one place it is actually true. Each rule sits
where it holds, instead of one type over-constraining every use.

Validation checks and moves on, leaving the illegal value representable and the
check re-runnable (and forgettable) downstream. Parsing produces a type that
carries the proof. Every later function taking a `Qty` is then relieved of
re-checking it — which is where "we validated it three layers up, mostly" bugs
come from.

Rules for our value types:

- **No `Deref` to the inner integer.** It undoes the newtype in one line and
  lets `Money` be added to `Px`.
- **`Copy` on the small ones** (`Qty`, `Px`, `Money`, ids) — they are integers.
- **Units live in the type.** `Money + Px` must not compile. That is the point.
- **No `Default`** where a default is meaningless. `Qty::default() == 0` is a
  footgun waiting for a struct-update expression.

## Make illegal states unrepresentable

An enum carrying its data, not a struct of `Option`s where some combinations are
nonsense ([Minsky](https://blog.janestreet.com/effective-ml-revisited/)):

```rust
// A struct with `filled_qty`, `cancelled_at` and `reject_reason` all Option
// admits "cancelled AND rejected with a fill" — a state no order can be in,
// which some handler will eventually construct and no test will cover.
pub enum OrderState {
    New,
    Acknowledged { broker_id: BrokerOrderId },
    PartiallyFilled { broker_id: BrokerOrderId, filled: Qty, cost: Money },
    Filled { filled: Qty, cost: Money },
    Cancelled { filled: Qty, cost: Money }, // may be non-zero — cancel after partial
    Rejected { reason: RejectReason },
}
```

The data each state carries is the data that state actually has. `Filled` has no
`reject_reason` because there is no such thing. And the fills accumulate `cost`,
not an average price — a stored, rounded average re-multiplied later is the
drift bug `code-style.md` exists to prevent, so it is not representable here
either.

## Typestate vs runtime enum — the order lifecycle

A real fork, decided here so it is not re-litigated during implementation.

**Typestate** encodes the state in the *type* — `Order<New>`, `Order<Acknowledged>` —
so `cancel()` exists only on the states where it is legal and an illegal
transition **fails to compile** ([Microsoft's Rust patterns](https://microsoft.github.io/RustTraining/rust-patterns-book/ch03-the-newtype-and-type-state-patterns.html)).
Genuinely elegant, and the strongest guarantee available.

**We are not using it for the order lifecycle**, for one disqualifying reason:
**our order state is loaded from an event log at runtime.** Typestate requires
the caller to know the state statically. A store replaying events cannot return
`Order<Acknowledged>` because which type to construct is not known until the row
is read — it would have to erase back to an enum at the boundary, and then we
have both mechanisms and the guarantee of neither.

**Decision: a runtime `OrderState` enum plus a total transition function**, with
exhaustiveness enforced by `match` and **no wildcard arm**:

```rust
pub fn apply(
    state: &OrderState,
    ordered: Qty,          // the order's total — decides partial vs complete
    event: &OrderEvent,
) -> Result<OrderState, TransitionError>
```

The compiler still catches the mistake that actually happens — adding a state or
an event and not handling every pairing — while the type stays constructible
from the log. We take the guarantee where it is available and decline to pay for
it where it is not.

Typestate remains right where the caller *does* know the state statically: a
builder, or a connection handshake. The general guidance is typestate for
compile-time lifecycle guarantees, enums for runtime uncertainty, and plain
validation where encoding costs more than it buys. Ours is runtime uncertainty.

**A wildcard `_ =>` arm in a transition match is a review failure.** It is the
one place exhaustiveness is doing real work, and a wildcard silently disables it.

## Errors

- **`thiserror` in libraries** (`domain`, `scoring`, `broker`, `engine`, `store`) — typed
  and matchable, so a caller can distinguish "rejected" from "broken".
- **Binaries return `Box<dyn Error>` from `main`**, where the answer is a
  message and a non-zero exit. `anyhow` was considered and declined: at two
  small binaries it is a dependency that buys a prettier backtrace and nothing
  else — `principles.md` §7's forwarding-layer test, applied to a crate.
- **Distinguish a bug from an expected failure.** A rejected order is a
  `Result` — it is a normal outcome of a competition. A negative position is a
  **bug**: `debug_assert!` plus a hard error, never a warning. This is
  `principles.md` §6's fail-fast, and the `baseline.md` §2 override.
- **No `unwrap()` outside tests.** `expect("reason")` only where an invariant
  makes it provable, and the string is the proof, not a shrug.
- Enforced: `unwrap_used` and `expect_used` denied in `domain` via workspace
  lints, not by review.

## Traits only at ports

Traits exist at the seams the dependency table names — `Broker` and `EventLog`
— and nowhere else. There is deliberately **no `Clock` trait**: time crosses the
boundary as a `Timestamp` argument, and a value needs no port. A trait would be
an abstraction over something the core is not allowed to do in the first place.

- **Static dispatch by default** (generics); `dyn` where a port is swapped at
  runtime or monomorphisation bloat is real.
- **No trait-per-struct.** A trait with exactly one implementation and no test
  double is not an abstraction, it is indirection — `principles.md` §5's
  declined-OCP in concrete form.
- Small ports over one god-trait. `EventLog` appends and reads; it does not also
  know about participants.

## Serde does not touch the domain

**Domain types carry no `#[derive(Serialize)]`.** DTOs live in `api` and mirror
them with explicit `From` impls.

This is the dependency rule applied to serialisation. A wire format is a
contract with the outside world; deriving it onto domain types couples the two,
turns an internal rename into a breaking API change, and quietly invites a field
into the JSON that was never meant to be public. It is also what lets money
serialise as a decimal **string** at the edge while staying an `i64` inside,
without a serde attribute anywhere near `domain`.

The extra `From` impls are the price. They are mechanical, they are tested by
the API tests, and they are the seam that lets the wire format change without a
domain commit.

## Derives

`Debug` on everything — it is what a failing assertion prints. `PartialEq`/`Eq`
on domain values so tests can compare them. `Clone` deliberately, not reflexively.
`Copy` on the small value types. `Default` only where zero is genuinely the
identity.

## Tests

- **Unit tests in the same file**, `#[cfg(test)] mod tests` — they can reach
  private internals, and they sit where a reader looking at the function will
  find them.
- **Integration tests in `tests/`**, public API only — the full-trading-day and
  replay-determinism tests from `build-order.md`.
- **Property tests (`proptest`) where the input space is the point**:
  display→parse round-trips across the full `Money` range, and `notional`
  checked against `i128` reference arithmetic, so a scale bug cannot hide.
  Fixed-case invariants — the wire round-trip, a closed position keeping zero
  basis — stay ordinary unit tests, because one well-chosen case pins them.
- **Test names are sentences.** `cancel_after_partial_fill_retains_filled_qty`,
  not `test_cancel_2`. They are read by the reviewer as documentation.

Three rules about what to test:

- **Fakes, not mock frameworks.** A port has few methods, so an in-memory
  `FakeEventLog` or a scripted `FakeBroker` is a dozen lines and behaves like the
  real thing. A mock asserting *which methods were called in what order* tests
  the implementation, which means every refactor breaks it — connascence of
  algorithm between test and code (`principles.md` §4).
- **Test behaviour, not structure.** Assert on resulting state and emitted
  events, never on private fields or call counts. The question a test answers is
  "does submitting this order produce this position", not "does it call
  `apply()` twice".
- **Do not test the compiler.** No test that a `Qty` cannot be negative when the
  constructor makes that unrepresentable — §3's whole point is that such a test
  has nothing to assert. Test the *parsing boundary* instead: that bad input is
  rejected with the right error.

## Workspace hygiene

- `[workspace.dependencies]` — every version declared once; members take
  `workspace = true`. One place to bump, no skew between crates.
- `[workspace.lints]` — lint policy once, inherited.
- `#![forbid(unsafe_code)]` in every crate. There is no reason for `unsafe` in
  this system, and saying so in the crate root makes that a compiler-enforced
  fact rather than a habit.
- Before done: `cargo fmt` · `cargo clippy --all-targets -- -D warnings` ·
  `cargo test`. Frontend: `npx vue-tsc --noEmit`.

## Naming

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/):
`as_*` borrow-to-borrow, `to_*` expensive conversion, `into_*` consuming;
`new` for infallible construction and a `Result`-returning `new`/`try_from` where
it can fail; `iter`/`iter_mut`/`into_iter` for the three iteration forms.

Idiom is a form of decoupling: code that reads the way a Rust programmer expects
costs the reviewer no translation.
