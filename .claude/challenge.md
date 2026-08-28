# The brief

Verbatim requirements from Unlok, plus what they actually score. **This file is
the requirement.** We do not get to redefine it, and anything not in it is not
in scope until the operator says so.

## Objective

> Design and implement a small backend service for a paper trading competition.
> The exercise should take approximately 1–2 days. Focus on **correctness,
> clarity, and practical engineering decisions rather than production
> completeness.**

## Requirements

**Order management.** Submit, cancel, replace/modify. States: `NEW`,
`ACKNOWLEDGED`, `PARTIALLY_FILLED`, `FILLED`, `CANCELLED`, `REJECTED`.
Implement a **mock broker** that generates execution reports, including partial
and complete fills.

**Portfolio tracking**, per participant: active orders · cash balance · current
positions · average position price · realized P&L · unrealized P&L · total
portfolio value. *May be limited to long positions in stocks.*

**Daily competition results.** At end of trading day, per participant: daily
P&L · daily return % · closing portfolio value. Generate a **daily leaderboard**
and an **overall competition ladder**.

We must **decide and document**:

- how daily winners are ranked
- how ties are resolved
- whether the overall ladder is cumulative P&L, percentage return, daily ranking
  points, or another method
- how inactive participants are handled

> The ranking should be **deterministic and fair between participants.**

**Interfaces.** A simple way to: create participants · submit/cancel/replace
orders · generate mock executions · update market prices · view orders and
positions · close a trading day · view daily and overall rankings.
*REST API, CLI, or application service all acceptable. **No user interface is
required.***

**Testing.** Unit tests covering the main flows: order submission and
acknowledgement · partial and complete fills · cancellation and replacement ·
position and P&L updates · daily leaderboard ranking.

## Deliverables

Source code · unit tests · instructions for running · short README.

The README must briefly explain: **architecture · key design decisions · P&L and
ranking approach · assumptions and limitations · what would change for
production.**

## Left open, deliberately

Language · framework · architecture · API design · data storage · event
handling · concurrency model · error handling · project structure · ranking
methodology · additional features.

> Use of AI tools is allowed, **but you should be able to explain and modify all
> submitted code.**

## What this actually scores

The open list above is the exam. They are not testing whether we can write a
CRUD service — they are testing **which decisions we make and whether we can
defend them.** Reading the requirements this way:

| They ask for | They are testing |
|---|---|
| Order states + mock broker | Do we model a lifecycle as a state machine with illegal transitions rejected, or as a mutable status string? |
| Replace/modify | Do we know cancel-replace semantics — that filled quantity survives a replace and a replace can race a fill? |
| Average position price, realized vs unrealized | Do we know cost-basis accounting, and do we keep money off floating point? |
| "Deterministic and fair" ranking | Do we produce a **total order** with documented tiebreaks, or a sort that can permute between runs? |
| "How inactive participants are handled" | Have we noticed that a flat participant beats every loser under return ranking, and did we decide that on purpose? |
| Unit tests of the listed flows | Is the domain logic pure enough to test without standing up a server? |
| "Explain and modify all code" | Is it small and clear enough to walk through live? |

**Scope discipline is itself scored.** The brief says 1–2 days and "not
production completeness". Gold-plating past it is a negative signal, not a
bonus. Where we do exceed the brief — the Vue cockpit is the standing example,
since "no user interface is required" — it ships **after** the scored work is
complete and is labelled as extra in the README.
