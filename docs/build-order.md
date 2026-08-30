# Build order — engine first

> The principle from [`.claude/principles.md`](../.claude/principles.md) §8:
> **the engine is the product; everything else is a way to reach it.** Order
> lifecycle, position/P&L accounting and the broker are what the brief scores.
> HTTP, persistence, CLI and the cockpit are transport, and if the dependency
> rule holds they can be added later without changing a line of the core.
>
> Each stage lists what lands, **the test that closes it**, and what it unblocks.
> A stage is not done because the code exists — it is done when its test is green.

## Stage A — the engine

No HTTP, no database, no async, no clock, no cockpit. Everything here is a pure
library plus one loop, and every test runs in-process in milliseconds.

### A0 · Workspace and money

Cargo workspace with the crate skeleton from `design.md` §10, and the value
types: `Money`, `Px`, `Qty`, `Symbol`, `ParticipantId`, `ClientOrderId`,
`BrokerOrderId`, `Timestamp`.

**Closes when:** money arithmetic is exact under a property test (no
associativity loss, no float anywhere), decimal-string serialisation
round-trips, and `domain/Cargo.toml` lists no async runtime, HTTP, SQL, clock or
RNG dependency.

**Unblocks:** everything. Nothing else starts until the vocabulary is fixed.

### A1 · Order lifecycle

The state machine: the transition table from `design.md` §4, the two-id map, and
cancel-replace semantics.

**Closes when:** the **exhaustive transition matrix** is tested — for every
state × every event, either the specified transition or an explicit rejection,
with no case unasserted. Plus: cancel before ack; cancel from `PARTIALLY_FILLED`
retaining `filled_qty`; replace cancelling the residual and preserving filled
quantity; replace losing the race to a complete fill.

**Unblocks:** A3, A4.

### A2 · Position and P&L

The fold from `design.md` §7: average cost as a rational, realized on sell,
unrealized against a mark, total portfolio value, fee convention on both sides.

**Closes when:** average cost is correct across `buy → buy → sell → buy` with
fees on both sides, hand-computed; realized and unrealized are separated
correctly; a sell exceeding the position is rejected; a held symbol with no mark
is an error rather than a zero.

**Unblocks:** B2.

> **A1 and A2 are independent** — different files, no shared types beyond A0.
> Two people can hold them at once. This is the parallel-work test from
> `principles.md` §4 being cashed in on the first day.

### A3 · Mock broker

Execution report generation behind a `Broker` port: explicit mode (driven by
command) and seeded auto mode (ack → *n* partials → complete), plus the reject
policies.

**Closes when:** the same seed produces byte-identical execution reports across
runs; partials sum exactly to the order quantity with no rounding residue; each
reject trigger produces `REJECTED`. `rng` is a parameter — `thread_rng` appears
nowhere.

**Unblocks:** A4.

### A4 · Engine assembly

Command → decide → events → apply, behind the single-writer loop. The `Broker`
port is defined here. (`EventLog` ended up defined in `store` — the wrong side
of the seam, recorded as a production delta in the README; the consequence is
that `App`, which needs both, lives in `api`.)

**Closes when:** an in-process test drives a **full trading day** — create
participants, submit, partially fill, cancel, replace, fill, update marks, read
portfolios — with no server, no database and no sleeping. Rejected commands
produce zero events.

---

### ⛳ Gate: the engine is done

Every flow the brief lists under *Testing* is now covered, with no transport in
the repository. **This is the milestone that matters**: if the schedule breaks
here, what exists is still a defensible submission — a tested trading engine
with a written design — rather than a half-wired web service.

Nothing below changes a line inside Stage A. If it does, the dependency rule was
broken and that is the bug to fix first.

## Stage B — durable and scorable

**B1 · Store.** SQLite append-only `events`, projections rebuilt by replay.
*Closes when:* projections rebuilt from the log equal the live projections field
for field, and a redelivered event is a proven no-op.

**B2 · Scoring.** Daily results, leaderboard, ladder, eligibility — all of
[`ranking.md`](ranking.md). *Closes when:* every tiebreak level is exercised,
shuffled input produces a byte-identical ranking, inactive participants behave
as specified, and the §6 worked example reproduces exactly.

> **B1 and B2 are independent.** B2 depends only on A2's output types; B1 depends
> only on A4's events. Parallelisable.

## Stage C — transport

**C1 · API.** Axum handlers over engine commands, RFC 7807 errors, OpenAPI.
Handlers translate and delegate — no decisions. *Closes when:* every endpoint in
`design.md` §11 is reachable, an illegal transition returns `409` carrying the
current state, and the handler layer holds no logic worth unit-testing.

**C2 · CLI.** Thin driver over the same service layer: one command replays a
full competition day. *Closes when:* the reviewer can run it and read the
leaderboard without touching curl.

**Then:** finalise `README.md` — architecture, decisions, P&L and ranking,
assumptions and limitations, production delta. It is a graded deliverable, so it
gets written when the facts are settled, not before.

## Stage D — beyond the brief

**D1 · Vue cockpit.** Only after C and the README are complete, per
`decision-log.md` 2026-08-28. The deletion test governs it: removing `ui/` must
leave everything the brief asked for intact and green.

## Sequencing at a glance

```
A0 ──┬── A1 ──┐
     └── A2 ──┴── A3 ── A4 ──╢GATE╟──┬── B1 ──┐
                                     └── B2 ──┴── C1 ── C2 ── README ── D1
```

Critical path is A0 → A1 → A3 → A4. A2 and B2 are the branches to hand off if
two of us are working at once.
