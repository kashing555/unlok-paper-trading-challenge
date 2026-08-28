# Working here: the deliverable is an argument

Extends `baseline.md` §1 (Think Before Coding) for a repo whose output is read
by an interviewer rather than run against a venue.

## The reviewer reads the README and the tests first

Assume they have 20 minutes. They will read `README.md`, skim the test names,
and open two or three source files. Optimise for that path:

- A test name is documentation. `partial_fill_then_cancel_leaves_filled_qty_intact`
  earns its length; `test_order_3` does not.
- If a design decision is not in the README or the decision log, it did not
  happen — they cannot award credit for reasoning they cannot see.
- Code they open should be the *interesting* code. Keep the domain small enough
  that the state machine and the P&L fold are findable in one jump.

## Every open decision gets recorded with its reasoning

The brief hands us a list of deliberately-unspecified choices — ranking method,
storage, concurrency, error handling. Each one is a scored answer.

Land it in `docs/decision-log.md`: **what was decided, what it was chosen over,
and the argument that decided it.** A decision without its alternative is
indistinguishable from a default nobody thought about.

Where the reasoning is a number — a tie rate, a rounding drift, a replay
mismatch — bring the number.

## Determinism is a testable property, not an aspiration

The brief says the ranking must be deterministic. That means we can assert it:

- Sorts end in a **total-order** tiebreak, so no two participants can compare
  equal. Test that shuffling input order does not change output ranking.
- Money is integer minor units. No float accumulates in a P&L path.
- The mock broker is seeded. Same seed, same execution reports, every run.
- Replaying the event log reproduces state exactly. That is a test, not a claim.

Anywhere the answer could depend on hash iteration order, wall-clock, or
floating point, treat it as a bug already present.

## Build the domain before the transport

The state machine, the position fold and the ranking are the scored content.
HTTP handlers and Vue components are how they are reached. Write and test the
first group standing alone — no server, no async, no store — then wire it up.

If a rule needs a running server to test, it is in the wrong layer.

## Do not exceed the brief without saying so

Scope past 1–2 days is a negative signal. Where we deliberately go further —
the cockpit — it lands after the scored work, is labelled extra in the README,
and never becomes a dependency of anything the brief asked for. The service
must remain complete and demonstrable with the UI deleted.

## Report faithfully

- If a test fails, say so with the output.
- If a step was skipped, say that.
- Silence is not success — "I didn't see an error" is not "it worked".
- Don't report done on code that hasn't been compiled and run.
