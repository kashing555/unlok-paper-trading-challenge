# unlok-paper-trading-challenge

Backend service for a **paper trading competition**: order management with a
mock broker, per-participant portfolio and P&L tracking, and deterministic daily
/ overall rankings.

Rust workspace (Axum HTTP) + Vue 3 cockpit. Long-only equities.

> **Status: in progress.** The design is settled and the engine is being built
> bottom-up — see [`docs/build-order.md`](docs/build-order.md) for the stages and
> what closes each one.
>
> | Stage | | |
> |---|---|---|
> | **A0** value vocabulary — money, ids, trading day | `crates/domain` | ✅ done |
> | **A1** order lifecycle state machine | `crates/domain` | ✅ done |
> | **A2** position and P&L | | next |
> | **A3** mock broker · **A4** engine loop | | |
> | B store · scoring — C API · CLI — D cockpit | | |

## Documentation

| | |
|---|---|
| [docs/design.md](docs/design.md) | Architecture and every design decision |
| [docs/ranking.md](docs/ranking.md) | P&L and ranking methodology, with a worked example |
| [docs/build-order.md](docs/build-order.md) | Build stages and what closes each |
| [docs/decision-log.md](docs/decision-log.md) | The dated ledger — every decision with the alternative it beat |

The brief itself is [`.claude/challenge.md`](.claude/challenge.md).

## Running it

```bash
cargo test --workspace
```

Build and run instructions land with the API in Stage C. The full README —
architecture, key design decisions, P&L and ranking approach, assumptions and
limitations, and what would change for production — is a graded deliverable and
is written once the code settles, so that it describes what exists rather than
what was planned.
