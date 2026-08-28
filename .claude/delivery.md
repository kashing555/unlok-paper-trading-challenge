# Delivering this

Extends `approach.md`. That file covers how to *work*; this one covers what is
actually handed over, because the submission is a larger artifact than the
source tree.

## The git history is part of the submission

The reviewer will run `git log`. It is the only record of *how* the thing was
built, and it is read as evidence of how we work.

- **Atomic commits.** One logical change each. **Every commit compiles and its
  tests pass** — a reviewer may check out any point in the history, and a broken
  intermediate commit is a broken promise about our discipline.
- **The message explains why.** The diff already shows what. Same rule as the
  comment convention in `code-style.md`: the reasoning and what it replaced.
- **The history should tell the build-order story.** Someone scrolling it should
  see the engine land, tested, before any HTTP appears — the same argument
  `build-order.md` makes, demonstrated instead of asserted.
- **No `wip`, no `fix`, no commits that break the build.** Equally: do not squash
  the whole exercise into one commit, which hides every decision we want seen.

## Write nothing we cannot defend

The brief's own constraint: *"you should be able to explain and modify all
submitted code."* That is a hard limit on what may ship, and it binds hardest on
anything generated rather than typed.

- Every dependency in `Cargo.toml` is one we can justify in a sentence and whose
  role we can describe. An unexplained crate is worse than the code it saved.
- Every abstraction answers "what breaks if this is deleted?" If the answer is
  "nothing", delete it — that is `principles.md` §7 with teeth.
- Generated code is read line by line before it is committed, at the same
  standard as code we wrote. Committing something we have not read is the one
  failure mode that turns an allowed tool into a disqualifying one.

**The test:** pick any file at random and explain, in two sentences, why it
exists and what would break without it. If that stalls, the file is the problem.

## Cutting scope under time pressure

The schedule will slip somewhere. Cut in a fixed order so the decision is made
now, calmly, rather than at midnight on day two:

1. **D1, the cockpit** — beyond the brief by definition.
2. **C2, the CLI** — the API already satisfies "a simple way to".
3. **Optional polish** — OpenAPI extras, extra broker fill policies.

**Never cut, in any circumstance:** tests (a listed deliverable), the README (a
listed deliverable), or anything inside Stage A (the scored content).

Two rules about how to cut:

- **Cut from the end, never from the middle.** A missing layer is a documented
  decision; a half-wired one is a bug, and reads as one.
- **Cut scope, never rigour.** Fewer features fully tested beats more features
  partially tested — that is the brief's own "correctness and clarity rather
  than production completeness", and it is being graded.

Anything cut goes in the README's limitations section, stated plainly as a
decision with its reasoning. Owning a gap costs nothing; being caught with an
unmentioned one costs everything.

## The README is graded, so it is written last

It is an explicit deliverable with a required structure — architecture, key
design decisions, P&L and ranking, assumptions and limitations, production
delta. Written after the code settles, from `docs/` and the decision log, so it
describes what exists rather than what was planned.

Assume it gets twenty minutes and is the first thing read. It should be
skimmable, honest about limits, and make the reasoning visible without
requiring the source.
