# Code conventions

Extends `baseline.md` §3 — "match existing style" means the following here.

## Money and prices are integer minor units. Never float.

Cash, prices, notionals and P&L are `i64` in minor units (cents) behind newtypes
(`Cash`, `Px`, `Qty`), constructed at the edge and never unwrapped into `f64`
inside a calculation. Serialise as a decimal string, not a JSON number — a
float round-trip through JSON is how a cent goes missing.

Average cost is the one genuinely fractional quantity. Hold it as a rational
(`total_cost: i64, total_qty: i64`) and divide only for display, rather than
storing a rounded average and re-multiplying it — that path drifts, silently,
and only shows up as a P&L that fails to reconcile at the end of the week.

## The execution report is the source of truth for position and P&L

Positions, cash and P&L are **projections of an append-only event log**, never
independently maintained beliefs kept in sync alongside it. Two copies of the
same fact are permitted to disagree, and eventually will.

This is the one architectural rule that everything else follows from. Do not add
a field that caches something derivable from the log without a measured reason
and a comment saying what it was.

## Illegal state transitions are rejected, not logged

The order lifecycle is a state machine with an explicit legal-transition table.
An unlisted transition returns an error and leaves state untouched. It never
warns and proceeds — a `FILLED` order that accepts another fill is a P&L bug
that surfaces days later on a reconciliation, not now.

## Pure decision logic in tested functions; I/O stays a thin shell

The state machine, the position fold, the P&L calculation and the ranking are
pure: no async, no store, no HTTP. Handlers do I/O and call them.

The test is whether a rule can be unit-tested without standing up a server. If
not, it is in the wrong layer.

## Sorts carry a total-order tiebreak

Every ranking sort ends in a field that is unique per participant, so no two
rows can compare equal and no two runs can disagree. Never sort a `HashMap`
iteration; collect and sort explicitly. See `docs/ranking.md`.

## Comments record WHY — the decision and what it replaced

Not "calculate realized pnl", but the reasoning that set the shape:

```rust
// Realized P&L books against the *running* average cost at the moment of the
// sell, not the average over the whole day: a participant who buys 100 @ 10,
// sells 50 @ 12, then buys 100 @ 14 has realized +100 — recomputing the basis
// afterwards would retroactively change a number already reported on a closed
// day's leaderboard. Closed days are immutable (decision-log 2026-08-28).
```

Everywhere else, less code. This is the one place more prose is correct.

## Ids: ours is minted before the broker sees the order

`client_order_id` is minted the instant we decide to submit, before any ack.
`broker_order_id` is recorded when known. The map between them is what makes
cancel-before-ack possible and makes a fill correlatable when the ack races the
execution report. Both ids appear on every event.

## Before saying done

`cargo fmt` · `cargo clippy` clean · `cargo test` green.
Frontend: `npx vue-tsc --noEmit`.

Don't report success on code you haven't compiled and run.
