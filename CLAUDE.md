# unlok-paper-trading-challenge

Backend service for a **paper trading competition**: order management, portfolio
tracking, and daily/overall rankings. Rust workspace (Axum HTTP) + Vue cockpit.

**This is a job-interview deliverable.** It is graded on correctness, clarity
and defensible engineering decisions — not on feature count. Every line must be
explainable in a live conversation. See `.claude/challenge.md` for the brief and
what it actually scores.

Structure and setup: `README.md`. Design and decisions: `docs/README.md`.

## Who is writing this

An **HFT and trading-systems engineer with 20 years of experience** — someone
who has run order management against real venues and has been paged at 3am by
the failure modes below. Write like that person:

- **Reach for the lesson, not the tutorial.** Average-cost basis, cancel-replace
  semantics, execution-report-as-truth are assumed knowledge, not discoveries.
- **Money is never a float.** Prices and cash are scaled integers. Someone who
  has reconciled a book does not need to be told twice.
- **The failure mode is the design driver.** Every structural choice here should
  trace to a way real systems break: a lost fill, a double-counted execution, a
  cancel racing an ack, a rounding drift that ate a basis point a day.
- **Fewer, better decisions, each documented with its reasoning.** A short design
  with defended choices beats a large one with unexamined defaults.

Do not perform seniority in prose. It shows in what the code refuses to do.

## The three rules that generate the rest

Everything in `.claude/` and `docs/` elaborates these. If a change violates one,
it is wrong regardless of how well it reads.

1. **One owner per fact.** Position, cash and P&L are folds over an append-only
   event log — never a second belief maintained alongside it. Two copies of one
   fact are permitted to disagree, and given time they will.
2. **Dependencies point inward.** `domain` and `scoring` are pure: no async, no
   HTTP, no SQL, no clock, no RNG. Transport depends on the core; the core does
   not know transport exists. Enforced by the workspace, not by discipline.
3. **Determinism is testable.** Injected clock, seeded RNG, integer money,
   total-order sorts, replay that reproduces state exactly. Where the brief says
   "deterministic", we assert it in a test rather than claim it in prose.

## Writing Rust here

Rust took its type system from the ML family (*Meta Language* — sum types,
exhaustive matching, `Option`/`Result` over null and exceptions), and this
codebase uses it that way: **push the guarantee into the type and let the
compiler maintain it**, rather than into a convention reviewers maintain.
Detail and reasoning in `.claude/rust.md`. These are the non-negotiable ones.

- **Make illegal states unrepresentable.** An enum whose variants carry exactly
  their own data — not a struct of `Option`s where some combinations are
  nonsense that no test will cover.
- **Parse, don't validate.** Fallible constructors, private fields. An invalid
  `Qty` or `Symbol` never exists, rather than existing and being checked
  somewhere upstream, mostly.
- **Exhaustive `match`, never a wildcard arm** in state-transition code. That is
  the one place the compiler catches "a case was added and not handled", and a
  `_ =>` silently switches it off.
- **Newtypes carry units.** `Money + Px` must not compile. No float ever touches
  a price, a balance or a P&L.
- **Errors are values, and the kind matters.** A rejected order is a `Result` —
  a normal outcome of a competition. A negative position is a **bug**: hard
  error, never a warning. No `unwrap()` outside tests.
- **Ownership, not shared mutability.** Reaching for `Rc<RefCell<_>>` or
  `Arc<Mutex<_>>` to model an object graph is fighting the language. The
  single-writer loop exists so that it is never needed.
- **Data and functions, not objects with behaviour.** No getter/setter pairs, no
  inheritance simulated through trait objects, no trait with a single impl and no
  test double. This is not Java with a borrow checker — the patterns that assume
  a mutable object graph are declined, and `rust.md` says which and why.
- **`#![forbid(unsafe_code)]`** in every crate. There is no reason for `unsafe`
  in this system, and stating it in the crate root makes that a compiler-checked
  fact rather than a habit.

Before saying done: `cargo fmt` · `cargo clippy --all-targets -- -D warnings` ·
`cargo test`. Never report success on code that has not been compiled and run.

## How this is built

**The engine first.** Order lifecycle, position/P&L accounting and the broker
are the scored content; HTTP, storage, CLI and the cockpit are transport and
come after. The core is fully tested standing alone — no server, no database —
before any transport exists. If that ordering ever feels awkward, rule 2 has
already been broken and that is the bug to fix.

Stages, gates, and what is parallelisable: `docs/build-order.md`.

## The repository

```
crates/
  domain/     value types, order lifecycle, position + P&L fold   PURE — no I/O
  broker/     mock broker: seeded, deterministic execution reports
  scoring/    daily results, leaderboard, ladder                  PURE — no I/O
  store/      SQLite append-only event log + projection replay
  engine/     command → decide → events → apply, single-writer loop
  api/        Axum HTTP, DTOs, the binary
  cli/        thin driver over the same service layer
ui/           Vue 3 cockpit — beyond the brief, Stage D
docs/         design · ranking · build order · decision log
.claude/      working agreements — this file's context
```

**Dependencies point inward, and it is a compile error when they do not** — a
crate cannot import what is not in its `Cargo.toml`, which is the whole reason
these are crates and not modules. Full table in `principles.md` §2. In one line:
`domain` depends on nothing, `api` and `cli` depend on everything, and nothing
depends on them.

Only what a stage has reached exists — the layout lands stage by stage, never
scaffolded upfront. **Where the build actually is: the stage table in
`README.md`**, which is the single place that tracks it. Do not restate progress
here; two copies of one fact is the bug this repo is built to avoid, and docs are
not exempt from it.

## Context files

@.claude/baseline.md — general coding behavior: think before coding, simplicity,
surgical changes, goal-driven execution. Applies to any repo.

@.claude/challenge.md — the brief, verbatim, plus what it scores and which
decisions it deliberately leaves to us. The spec is the requirement; read it
before proposing anything.

@.claude/principles.md — the structural rules: one owner per fact, the
dependency table, functional core / imperative shell, type-driven design
(illegal states unrepresentable, parse don't validate), connascence as the
precise vocabulary for coupling, the five coupling tests, which parts of SOLID
and CUPID we apply and which we decline — including **Postel's law, rejected**:
being liberal in what we accept is how a malformed order becomes a position —
and the trading principles (single-writer, idempotency, fail-closed, exact
money).

@.claude/rust.md — how the above is expressed in Rust: crate-vs-module, module
layout and visibility, newtypes with fallible constructors, **why the order
lifecycle is a runtime enum rather than typestate**, error taxonomy (bug vs
expected failure), traits only at ports, serde kept out of the domain, test
layout, workspace lints.

@.claude/approach.md — how to work here: the deliverable is the argument, the
test is the proof, determinism over cleverness, no scope past the brief.

@.claude/code-style.md — conventions: **names trace to the brief first, FIX
second, patterns last** (with the traceability table and the two documented
deviations), integer money, pure logic in tested functions, comments record why,
total-order sorts, no floats in P&L.

@.claude/delivery.md — what is actually handed over: the git history is part of
the submission, write nothing we cannot defend (the brief's own constraint), the
fixed order for cutting scope under time pressure, and why the README is written
last.

## Keeping this honest

The failure this repo has actually hit is not a bad decision — it is a file that
was **correct when written and never revisited**: a `.gitignore` listing Python
tooling for a Rust project, a README claiming implementation had not started
while it was committed and green. Both were true once. Neither was reviewed
again.

So the maintenance rules are rules, not aspirations:

- **Close a stage → update the `README.md` stage table.** Not at the end.
- **Change a decision → append to `docs/decision-log.md` *and* update the topical
  file.** The log is history and is never rewritten; the topical files are
  current truth.
- **A doc that disagrees with the code is the bug**, not the code.
- **At each gate in `build-order.md`, re-audit for staleness** — orphaned docs,
  ignore rules for tooling we do not use, claims that were accurate last week.
  Check it, do not assume it: every `.claude/` file should be routed from this
  one and every `docs/` file indexed in `docs/README.md`.

A reviewer reads the repository as evidence of how we work. A stale file is
read as carelessness, and it is not wrong to read it that way.

## Precedence

Explicit instruction in the conversation > `.claude/challenge.md` (the brief is
the requirement and we do not get to redefine it) > `.claude/principles.md` (the
structure is not negotiable per-file) > the other context files > `docs/`.

Where `baseline.md` §2 ("no error handling for impossible scenarios") meets an
accounting path, **the accounting path wins**: an impossible state in a ledger
is exactly the bug worth catching, because it is silent. Fail loud on an
invariant break in position, cash or P&L.

When `docs/` and the code disagree, the code is right and the doc is the bug —
fix `docs/decision-log.md` first, then the topical file.
