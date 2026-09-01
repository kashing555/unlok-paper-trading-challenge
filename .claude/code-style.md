# Code conventions

Extends `baseline.md` §3 — "match existing style" means the following here.

## Money and prices are scaled integers. Never float.

Money, prices, notionals and P&L are `i64` at one shared 1e4 scale behind
newtypes (`Money`, `Px`; `Qty` is unscaled whole shares), constructed at the
edge and never unwrapped into `f64` inside a calculation. The shared scale is
what makes `notional = px × qty` an exact multiply — no division, so no rounding
in the core (`docs/design.md` §3). Serialise as a decimal string, not a JSON
number — a float round-trip through JSON is how a cent goes missing.

Average cost is the one genuinely fractional quantity. Hold it as a rational
(the position's `cost` over its `qty`) and divide only for display, rather than
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

## Names come from the brief first, FIX second, patterns last

Every domain name in this repo must trace to a source. In priority order:

1. **The brief's word wins.** It is the ubiquitous language of the exercise, and
   the reviewer reads the code against the requirements they wrote. If they said
   *cash balance*, the field is `cash` — not `available_funds`, not `liquidity`.
2. **Where the brief is silent, FIX.** This is a trading system and FIX is its
   dictionary. Using its vocabulary means a reader who knows the protocol needs
   no translation, and the tag number makes the choice checkable.
3. **Where both are silent, the standard pattern name.** `Money` is Fowler's
   Money pattern, not a coinage.

Never invent a synonym for a term the brief already uses. Never abbreviate one.

### The traceability table

| Name in code | Source | Exactly |
|---|---|---|
| `ParticipantId` | brief | "each participant" |
| `cash: Money` | brief | "**cash balance**" |
| `Position` | brief | "current positions" |
| `avg_price` | brief | "average position price" |
| `realized_pnl` / `unrealized_pnl` | brief | "realized P&L", "unrealized P&L" |
| `total_value` | brief | "total portfolio value" |
| `daily_pnl` / `daily_return` / `closing_value` | brief | "daily P&L", "daily return percentage", "closing portfolio value" |
| `Leaderboard` / `Ladder` | brief | "daily leaderboard", "overall competition ladder" |
| `OrderState::{New, Acknowledged, PartiallyFilled, Filled, Cancelled, Rejected}` | brief | the six states, verbatim |
| `Execution` (broker) | brief + FIX | "execution reports"; FIX `MsgType=8` |
| `TradingDay` | brief | "trading day" |
| `ClientOrderId` | FIX | `ClOrdID`, tag 11 |
| `BrokerOrderId` | FIX | `OrderID`, tag 37 |
| `replaces` | FIX | `OrigClOrdID`, tag 41 |
| `ExecutionId` | FIX | `ExecID`, tag 17 — one per fill |
| `Px` | FIX | `Price` 44, `LastPx` 31, `AvgPx` 6 |
| `Qty` | FIX | `OrderQty` 38, `CumQty` 14, `LeavesQty` 151 |
| `Side` | FIX | `Side`, tag 54 |
| `Money` | pattern | Fowler, *Money* (PoEAA) |

### The two deliberate deviations, and why

- **`Money`, where the brief says "cash".** *Cash* is one **use** of the type —
  the uninvested balance — and the brief uses it that way. But the same type
  also carries a notional, a fee and a P&L, and a fee is not cash. So the
  **type** is `Money` and the **field** is `cash`, which is rule 1 satisfied
  where it applies rather than stretched past it.
- **`mark`, where the brief says "market price".** *Mark* is the standard term
  for the price a position is valued at, and it usefully distinguishes that from
  a price on an order. The brief's word is kept at the boundary — the endpoint
  stays `POST /market/prices` — and the domain word is used inside. Brief's
  language at the edge, domain language in the core.

Both are written down here precisely because they are deviations. An undocumented
deviation is indistinguishable from carelessness.

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
execution report. Both ids appear on every event. On the wire nothing is ever called plain
`orderId`: in FIX, *OrderID* is tag 37 — the **broker's** id — so a JSON field
named `orderId` carrying the client id reads backwards to anyone fluent. The
wire speaks the trio in full: `clientOrderId` · `brokerOrderId` · `execId`.

## Before saying done

`cargo fmt` · `cargo clippy` clean · `cargo test` green.
Frontend: `npx vue-tsc --noEmit`.

Don't report success on code you haven't compiled and run.
