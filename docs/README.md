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

Also outside `docs/`: [`.claude/challenge.md`](../.claude/challenge.md) — the
brief verbatim and what it actually scores. Read it before proposing anything.
The rest of `.claude/` holds the working agreements.

---

**Maintenance rule.** When a discussion changes a decision, append to the
decision log *and* update the topical file. The log is history and is never
rewritten; the topical files are current truth.

**When a doc and the code disagree, the code is right and the doc is the bug.**
