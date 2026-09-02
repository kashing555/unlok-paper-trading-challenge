# Cockpit

A Vue 3 + TypeScript read/write console for the competition.

**Beyond the brief** — *"no user interface is required"* — and built after the
scored work was complete. The service is fully usable with this directory
deleted; nothing in `crates/` depends on it.

```bash
cargo run                    # terminal 1 — the API on :8080
cd ui && npm install && npm run dev
```

The Vite dev server proxies `/api` to `127.0.0.1:8080`, so the browser sees one
origin and the Rust side needs no CORS layer — one less dependency in the part
of the system that is actually being scored. Override with `PTC_URL`.

```bash
npm run build       # vue-tsc --noEmit && vite build
npm run typecheck
```

## Two pages

**Console** is the operator's desk: all seven of the brief's interface
operations — create participants, submit/cancel/replace, drive executions,
update marks, view orders and positions, close a day, view both rankings.

**Simulation** runs a scripted two-day competition through the same
public REST endpoints the console uses — partial fill, a cancel that keeps its
fills, a cancel-replace, marks, two day closes, and carol who never trades —
with the narrated log on the left and portfolios/rankings filling in live on
the right. It adapts to live state: existing participants are reused and it
closes the next two unclosed days, so it composes with manual play and with
itself. No private endpoint backs it; what you watch is exactly what the
public API can do. (Navigation is a reactive switch, not vue-router — a
dependency for a boolean is the same declined forwarding-layer as elsewhere.)

## Notes

**Money and returns are kept as decimal strings and never parsed into a
`number`.** `pct()` formats percentages with `BigInt` string arithmetic rather
than `Number(x) * 100`, because the two disagree: a cumulative return of
`-0.0005055` renders as `-0.0505%` through a float and `-0.0506%` in the
backend, since the binary double lands just below the midpoint. A frontend that
rounds differently from the service it displays is a frontend that will be
trusted and be wrong. This was a real bug, caught by comparing the rendered page
against the API.

**Nothing is derived locally and kept in step.** The store polls and displays;
every number shown is one the backend computed, for the same reason the backend
does not maintain a second copy of anything either.

**The ranking rules come from the API payload** (`rankedBy`, `tiebreaks`,
`eligibility`) rather than being restated here, so the two cannot drift.
