# Paper Trading Challenge

A backend service for a paper trading competition: order management against a
mock broker, per-participant portfolio and P&L tracking, and deterministic daily
and overall rankings.

**Rust workspace, Axum HTTP API, SQLite event log. Long-only equities.**
98 tests. `cargo test --workspace` is green, `clippy -D warnings` is silent.

```bash
cargo run --bin ptc-demo     # a full two-day competition, start to finish
```

---

## Contents

- [Running it](#running-it) · [What the demo shows](#what-the-demo-shows)
- [Architecture](#architecture) · [Key design decisions](#key-design-decisions)
- [P&L](#pl) · [Ranking](#ranking)
- [API](#api) · [Testing](#testing)
- [Assumptions and limitations](#assumptions-and-limitations)
- [What would change for production](#what-would-change-for-production)

## Running it

Needs a stable Rust toolchain (developed and tested on 1.97; no nightly
features are used). Nothing else — SQLite is bundled, so there is no database to
provision and no system library to install.

```bash
cargo test --workspace       # 98 tests
./scripts/check.sh           # fmt + clippy -D warnings + tests
```

**The scripted demo** is the fastest way to see the whole system. It runs a
two-day competition through the same application layer the HTTP API uses, with a
seeded broker and supplied timestamps, so the output is identical on every run:

```bash
cargo run --bin ptc-demo
```

**The cockpit** (optional, and beyond the brief — the service is complete
without it):

```bash
cargo run --bin ptc                       # terminal 1
cd ui && npm install && npm run dev       # terminal 2, then open :5173
```

**The server:**

```bash
cargo run --bin ptc          # http://127.0.0.1:8080, log in ./ptc.sqlite
```

| Variable | Default | |
|---|---|---|
| `PTC_ADDR` | `127.0.0.1:8080` | listen address |
| `PTC_DB` | `ptc.sqlite` | event log path; `:memory:` for an ephemeral run |
| `PTC_SEED` | `42` | broker RNG seed — same seed, same fills |
| `PTC_FEE_BPS` | `0` | commission in basis points of notional |
| `PTC_MAX_SLICES` | `3` | partial fills an order is broken into |

A minimal walkthrough:

```bash
curl -sX POST localhost:8080/participants -H 'content-type: application/json' \
  -d '{"id":"alice","startingCash":"100000"}'

curl -sX POST localhost:8080/orders -H 'content-type: application/json' \
  -d '{"participant":"alice","symbol":"AAPL","side":"buy","qty":100,"limitPx":"10"}'

curl -sX POST localhost:8080/broker/executions -H 'content-type: application/json' \
  -d '{"orderId":1}'                       # broker chooses the terms

curl -sX POST localhost:8080/market/prices -H 'content-type: application/json' \
  -d '[{"symbol":"AAPL","px":"12"}]'

curl -s localhost:8080/participants/alice/portfolio
curl -sX POST localhost:8080/days/2026-08-28/close
curl -s localhost:8080/ladder
```

### What the demo shows

Three participants. Alice trades actively, Bob trades once and holds, **Carol
never trades at all** — the case the ranking rules have to answer.

It exercises every flow the brief lists: submission and acknowledgement, partial
and complete fills, a cancel that keeps its fills, a cancel-replace, position and
P&L updates, two day closes, and the overall ladder. The last three lines:

```
    rank  who        cumulative   wins  active  points  eligible
    1     alice         0.1417%      2       2      50  yes
    2     bob          -0.0506%      0       2      30  yes
    -     carol         0.0000%      0       0      36  no (never traded)
```

Carol's 0% beat Bob's loss on both days, and she is still unranked. That is
deliberate, and [Ranking](#ranking) explains why.

## Architecture

```
crates/
  domain/    value types, order lifecycle, position + P&L fold   PURE — no I/O
  broker/    mock broker: seeded, deterministic execution reports
  scoring/   daily results, leaderboard, ladder                  PURE — no I/O
  engine/    command -> decide -> events -> apply
  store/     SQLite append-only event log + replay
  api/       App (engine + log), Axum HTTP, `ptc` server, `ptc-demo` CLI
ui/          Vue 3 cockpit — beyond the brief, see ui/README.md
```

**Dependencies point inward, and it is a compile error when they do not** — a
crate cannot import what is not in its `Cargo.toml`. That is the reason these
are separate crates rather than modules: the rule is enforced by the toolchain
instead of by review.

`domain` and `scoring` hold no I/O at all. Their entire dependency tree is
`chrono` and `thiserror`, and `chrono` is declared without default features so
the `clock` feature is absent — **`Utc::now()` does not compile inside the
domain**. That is verified, not asserted.

```
participant ─┬─▶ submit / cancel / replace ─▶ engine ─▶ mock broker
             │                                  │           │
             │                                  ▼           ▼
             └────────── read ◀── projections ◀── EVENT LOG (append-only)
                                      ▲
market data ───── mark update ────────┘
```

Full design in [`docs/design.md`](docs/design.md); every decision with the
alternative it beat is in [`docs/decision-log.md`](docs/decision-log.md).

## Key design decisions

**The execution report is the source of truth.** Positions, cash and P&L are
**folds over an append-only event log**, never independently maintained beliefs
kept in step alongside it. Two copies of one fact are permitted to disagree, and
given time they will — that is the entire bug class this design removes. The
mock broker is the venue here, so there is no excuse for inferring a position we
were told about.

It pays off immediately: **replay is a test**, not a claim. Rebuilding from the
log must reproduce state exactly, and the tests do it with a broker on a
*different seed* — which only passes because replay never consults the broker.
Every broker decision is already a recorded fact.

**Money is exact, and never a float.** `Money`, `Px` and `Qty` are `i64`
newtypes. `Money` and `Px` share one scale of four decimal places and `Qty` is
unscaled, so `notional = px × qty` is an exact multiply — **no division, so no
rounding anywhere in the core**. On a cents scale it would have been
`px × qty / 100`, and `$10.0050 × 1 share` is 1000.5 cents: storable only by
rounding, on every fill, silently. Money crosses the wire as decimal **strings**,
because a JSON number is an IEEE double.

**Illegal states are unrepresentable, and illegal transitions are rejected.**
The six order states are an enum where each variant carries exactly its own
data — `Acknowledged` cannot exist without a broker id, `Rejected` has no filled
quantity. Transitions are a total function over `(state, event)` with **no
wildcard arm**, so adding a state or an event is a compile error rather than a
silently unhandled case. Verified by adding one and checking it fails to build.
A fill on a terminal order is an error, not a warning: it is a P&L bug that
would otherwise surface days later during a reconciliation.

**Two order ids, because FIX has two.** `ClientOrderId` is ours, minted before
submission (FIX `ClOrdID`, tag 11); `BrokerOrderId` is the broker's, recorded on
the ack (tag 37). This is load-bearing rather than ceremonial: executions are
driven separately from submission, so an order genuinely sits in `NEW` with no
broker id, and cancelling in that window is only expressible with an id we
minted. A registry keyed on the broker's id would also drop any execution report
arriving before the ack.

**Replace is cancel-replace.** Replacing an order that is 40/100 filled
withdraws the residual 60; the 40 stays booked against the original, and the
replacement carries a `replaces` link (FIX `OrigClOrdID`, tag 41). A replace
that loses the race to a complete fill is **rejected with the terminal state
named**, not silently turned into a second position.

**Pre-trade reservations are derived, never cached.** Available cash is the
balance minus the notional of working buy orders, recomputed from those orders
each time. A reserved-cash counter kept beside them would be a second copy of a
fact the orders already hold. Without it, two orders each pass at submit and the
second fails deep in the book at fill time — far too late.

**Events are durable before they are visible.** `plan` → persist → `apply`.
Applying first leaves state the log cannot reproduce if the process dies in
between.

**Fail closed.** A held symbol with no mark cannot be valued: closing a day in
that state is refused rather than publishing a book valued at zero. A *portfolio
read* still succeeds, with `totalValue: null` and a `valuationError` naming the
symbol — fail closed means never a wrong number, not never a response.

**Strict parsing; Postel's law is declined.** `10.123456` is rejected, not
truncated. `aapl` is rejected, not upper-cased — two spellings of one key is how
executions get filed twice. Being liberal in what we accept is how a malformed
order becomes a real position.

## P&L

Long-only, so the accounting is average cost with realization on sell.

```
buy  q @ p, fee f:   cash −= q·p + f
                     cost += q·p + f          (fees capitalised into the basis)
                     qty  += q

sell q @ p, fee f:   removed   = cost · q / qty        (i128, exact on the close)
                     realized += q·p − removed − f
                     cash     += q·p − f
                     cost     −= removed
                     qty      −= q
```

- **Unrealized** = `qty · mark − cost`. Deliberately *not* `qty · (mark − avg)`:
  algebraically identical, but that form needs the average and therefore a
  division. This one is exact.
- **Total portfolio value** = `cash + Σ(qty × mark)`.
- **Average cost is never stored** — only `qty` and `cost`, with the average
  derived for display. Storing a rounded average and re-multiplying it drifts,
  and the drift is invisible until a reconciliation fails. The tests pin the
  consequence: buy 100 @ 10 then 100 @ 12 gives an average of 11.05, and
  **selling 50 leaves it at 11.05**, because the sale removes basis and quantity
  in the same proportion.
- **Fees are capitalised on buy and expensed on sell** — one convention, both
  sides. Applying it on one side only leaks a fee per round trip.
- **Partial sales cannot accumulate rounding residue.** The basis removed is
  truncated, but a *final* sale has `sold == held`, so that division is exact and
  takes the whole remaining basis with it. Tested on 31.0000 over 3 shares: the
  intermediate sale leaves 20.6667, and the close leaves **exactly zero**.
- **Daily P&L** = today's closing value − yesterday's, not the sum of realized
  P&L, which would ignore mark-to-market on open positions. A participant's
  first day is measured from their starting cash.

## Ranking

The brief leaves this open and asks it to be **deterministic and fair**. Full
reasoning, with what each choice was made *over*, in
[`docs/ranking.md`](docs/ranking.md).

**Daily leaderboard — ranked on daily return %**, not absolute P&L. Absolute
P&L ranks the largest account rather than the best trading the moment capital
differs.

| Tiebreak | Direction | Why |
|---|---|---|
| daily return % | desc | the result itself |
| **turnover** | **asc** | the same return on less trading is the better result — less fee drag, less exposure |
| `participantId` | asc | **total-order guarantee** |

The last key is arbitrary and openly so. Its job is not fairness but
determinism: with it, no two participants can compare equal, so the ranking
cannot permute between runs. Asserted by ranking three different input orderings
of a fully-tied day and requiring identical output.

**Overall ladder — the geometric compound of daily returns**, `Π(1 + rᵢ) − 1`.
Chosen over cumulative P&L, which can be bought with size; and over the
arithmetic sum of returns, which is simply wrong — −50% then +50% *sums* to zero
but leaves you down 25%. One test pins exactly that case. Formula-1 style
ranking points are computed and shown as a **secondary column**, because they
reward consistency but discard magnitude; displaying the alternative is cheaper
than arguing about it.

**Inactive participants.** An inactive *day* ranks normally at 0% — choosing to
be flat is a real trading decision, and on a down day it wins. But **ladder
placement requires at least one active day**: a participant who never traded at
all is listed with `rank: null, eligible: false`. The line is between *choosing
to be flat* (a strategy, ranked) and *never having participated* (not a
competitor, unranked). Without it, an account that never places an order tops
the ladder in a falling market.

*Active* means at least one fill that day, or a non-zero position held.

**Determinism is a property we test:** integer money, `Decimal` returns at a
fixed scale, `BTreeMap` everywhere an ordered output is produced, total-order
sorts, and closed days that are immutable — re-closing returns the published
board rather than recomputing it.

## API

Errors are [RFC 7807](https://datatracker.ietf.org/doc/html/rfc7807)
`application/problem+json` with a stable machine-readable `type`, and the useful
ones carry the numbers that explain them.

| | | |
|---|---|---|
| `POST` | `/participants` | create, with starting cash |
| `GET` | `/participants` | list |
| `GET` | `/participants/{id}/portfolio` | cash, positions, average price, realized/unrealized, total value, active orders |
| `GET` | `/participants/{id}/orders` | full order history |
| `POST` | `/orders` | submit → `NEW` |
| `GET` | `/orders` · `/orders/{id}` | read |
| `DELETE` | `/orders/{id}` | cancel |
| `PUT` | `/orders/{id}` | replace — returns **both** sides |
| `POST` | `/broker/executions` | generate an execution; omit `qty`/`px` to let the broker choose |
| `POST` | `/market/prices` | update marks (batch) |
| `POST` | `/days/{day}/close` | close a trading day (idempotent) |
| `GET` | `/days` · `/days/{day}/leaderboard` | closed days, daily board |
| `GET` | `/ladder` | overall competition ladder |
| `GET` | `/health` | counts |

An illegal transition returns `409` **naming the current state** — "it is
already `FILLED`" is actionable where "that failed" is not:

```json
{ "type": "https://unlok-ptc.invalid/errors/illegal-transition",
  "status": 409, "currentState": "FILLED", "attempted": "a cancellation" }
```

The leaderboard and ladder payloads state their own ranking rules
(`rankedBy`, `tiebreaks`, `eligibility`), so a consumer never has to infer them.

## Testing

98 tests. The domain and scoring crates hold no I/O, so the scored logic is
tested without standing up a server; the integration tests then prove it
composes.

| | |
|---|---|
| Order lifecycle | the **full 6 × 4 state/event matrix**, with a length assertion so a new pair cannot escape coverage |
| Exhaustiveness | verified by adding a variant and confirming `E0004`, not by reading the code |
| Fills | partials accumulating, the fill that closes an order, overfill, empty fill, fill on a terminal order |
| Cancel / replace | cancel before ack, cancel keeping its fills, replace preserving filled quantity, replace losing the race |
| P&L | a hand-computed buy → buy → sell → buy with fees on both sides; `total = starting + realized + unrealized` as an invariant |
| Rounding | a basis that does not divide evenly closing to exactly zero |
| Ranking | the worked example from `ranking.md` §6 executed; every tiebreak; shuffled input producing identical output |
| Replay | rebuilt state equals live state, through SQLite, with a **different broker seed** |
| HTTP | every endpoint, RFC 7807 bodies, restart-from-log |
| Determinism | two demo runs diffed byte for byte |

## Assumptions and limitations

Stated as decisions, not gaps.

- **Long-only equities, one currency, no leverage or margin.** Permitted by the
  brief. A sell beyond the position is rejected at submit.
- **Whole shares.** No fractional quantities.
- **Marketable-limit fills only** — no order book, no queue position, no
  price-time priority, no price improvement. Fills are at the order's own limit
  price.
- **No settlement rounding.** Money is exact at four decimal places throughout;
  a real system rounds to a currency's minor unit at settlement.
- **A trading day is whatever the operator closes.** No exchange calendar, no
  holidays. Resting orders **carry over** a close — a working order is exposure
  the participant intends to have.
- **All participants are assumed to start together with equal capital.**
  Compounding a late joiner over fewer days is not directly comparable; a
  per-day geometric mean would normalise it.
- **No deposits or withdrawals after creation.** Daily P&L as a value difference
  is only valid without external cash flows.
- **No auth, no rate limiting, no multi-tenancy.** Every caller can act as any
  participant.
- **Ranking rules are code.** A `DayClosed` event stores the day's *facts* and
  the board is recomputed from them, so a stored board can never drift from what
  it was built on — but changing the ranking rules would change historical
  boards.
- **One process.** State is in memory, rebuilt by replaying the log at startup.

## What would change for production

- **Concurrency.** One mutex serialises every command today. That is the
  single-writer principle in its simplest form and it is right at this volume;
  the next step is a channel-and-actor so reads never block writes, then
  partitioning by participant.
- **A real venue.** The mock broker is synchronous and honest. A FIX or REST
  adapter makes acks asynchronous and unreliable, forcing timeouts, retries with
  idempotency keys, and an orphan-order sweep — the part of a real OMS this
  exercise legitimately omits.
- **Storage.** SQLite → Postgres, event log partitioned by date, projections
  materialised and checkpointed rather than replayed from zero at every boot.
- **The port boundary.** `EventLog` should be defined *in* `engine` and
  implemented by `store` — the consumer defining the port. Today `store` defines
  it, which is why `App` lives in `api`. Fixing it lets the application layer
  move below HTTP.
- **Market data.** Marks from a real feed, with staleness bounds and a halt on
  unreadable data rather than a manual endpoint.
- **Day close as a scheduled job**, against an exchange calendar, with a
  settlement cycle.
- **Ranking migrations**, so a rules change cannot alter already-published
  boards.
- **Auth, per-participant rate limits, and an audit trail** on every mutation.
- **Observability.** Structured logs and metrics; the event log already gives a
  complete audit trail, which is most of the way there.

---

## Notes on process

`CLAUDE.md` and `.claude/` hold the working agreements this was built under —
the structural rules, the coding conventions, and the naming rule (**every
domain name traces to the brief, then to FIX, then to a standard pattern**, with
a traceability table).

[`docs/decision-log.md`](docs/decision-log.md) is the dated ledger: every
decision with the alternative it beat, including the ones the implementation
proved wrong. Three examples, because being able to show where the design was
corrected is more useful than pretending it was right first time:

- The money scale was specified as cents; implementing it showed that makes
  notional inexact, and the design doc was corrected.
- The concurrency section specified a channel-and-actor; a mutex delivers the
  same ordering property with far less machinery, so the doc was corrected.
- The dependency table had `engine` depending on `store`, which is a cycle. It
  was corrected, and the right shape is filed as a production delta above.

AI assistance was used, which the brief permits. Every line has been reviewed
and is explainable.
