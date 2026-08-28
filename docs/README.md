# Documentation

**The ledger**

| File | Contents |
|---|---|
| [decision-log.md](decision-log.md) | **Every decision, dated, with what it was chosen over and the argument that decided it.** Append-only; new discussions land here first |

**The system**

| File | Contents |
|---|---|
| [design.md](design.md) | The whole design: event log as source of truth, money representation, order lifecycle, replace semantics, mock broker, P&L math, day close, crate layout, API, concurrency, storage, test plan, production delta |
| [ranking.md](ranking.md) | **The scored decision** — daily leaderboard, tiebreak chain, overall ladder, inactive participants, determinism, worked example |
| [build-order.md](build-order.md) | **Engine first** — the stages, the test that closes each, the gate after the engine, and what is parallelisable |

Also outside `docs/`: [`.claude/challenge.md`](../.claude/challenge.md) — the
brief verbatim and what it actually scores, read before proposing anything —
and [`.claude/principles.md`](../.claude/principles.md) +
[`.claude/rust.md`](../.claude/rust.md), the structural rules (dependency table,
type-driven design, connascence, coupling tests, trading principles) and their
expression in Rust, which the design here is an application of. The rest of `.claude/` holds the working agreements.

---

**Maintenance rule.** When a discussion changes a decision, append to the
decision log *and* update the topical file. The log is history and is never
rewritten; the topical files are current truth.

**When a doc and the code disagree, the code is right and the doc is the bug.**
