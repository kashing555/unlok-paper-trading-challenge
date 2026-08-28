# unlok-paper-trading-challenge

Backend service for a **paper trading competition**: order management with a
mock broker, per-participant portfolio and P&L tracking, and deterministic daily
/ overall rankings.

Rust workspace (Axum HTTP) + Vue 3 cockpit. Long-only equities.

> **Status: design complete, implementation not started.**
> Start with [`docs/design.md`](docs/design.md) and
> [`docs/ranking.md`](docs/ranking.md). The brief is
> [`.claude/challenge.md`](.claude/challenge.md).

## Documentation

| | |
|---|---|
| [docs/design.md](docs/design.md) | Architecture and every design decision |
| [docs/ranking.md](docs/ranking.md) | P&L and ranking methodology, with a worked example |
| [docs/decision-log.md](docs/decision-log.md) | The dated ledger of decisions |

## Running it

To be written alongside the implementation. This section is a deliverable — the
brief asks for instructions for running the project — and will cover build,
test, the scripted demo of a full trading day, and the cockpit.
