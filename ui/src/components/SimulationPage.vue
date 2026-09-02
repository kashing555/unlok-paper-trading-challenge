<script setup lang="ts">
import { reactive } from 'vue'
import { api, ApiError, type OrderView } from '../api'
import { useCockpit } from '../store'
import OrdersPanel from './OrdersPanel.vue'
import Portfolios from './Portfolios.vue'
import Rankings from './Rankings.vue'
import TapePanel from './TapePanel.vue'

const s = useCockpit()

// A scripted two-day competition, driven through the same REST endpoints as every
// button in this cockpit — no private path, so what you watch is exactly
// what the API can do. Adapts to live state: participants are created only
// if missing, and the two closed days are the next ones after whatever is
// already closed, so it can run on a fresh server or on top of manual play.
const run = reactive({ busy: false, log: [] as string[] })

const say = (m: string) => run.log.push(m)
const pause = (ms = 350) => new Promise((r) => setTimeout(r, ms))
const TERMINAL = ['FILLED', 'CANCELLED', 'REJECTED']

async function ensureParticipant(id: string) {
  try {
    await api.createParticipant(id, '100000')
    say(`create  ${id} with 100000`)
  } catch (e) {
    if (e instanceof ApiError && e.status === 409) say(`exists  ${id} — reusing`)
    else throw e
  }
}

async function autoFill(id: number): Promise<OrderView> {
  let last: OrderView | undefined
  for (let i = 0; i < 10; i++) {
    last = await api.execute(id)
    say(`fill    #${id} → ${last.state} ${last.filledQty}/${last.qty} · fees ${last.fees}`)
    await pause()
    if (TERMINAL.includes(last.state)) return last
  }
  throw new Error(`order #${id} did not terminate`)
}

/** Narrate, then let the panels on the right catch up. */
async function beat() {
  await s.refresh()
  await pause()
}

function nextDay(d: string): string {
  return new Date(Date.parse(d + 'T00:00:00Z') + 86_400_000).toISOString().slice(0, 10)
}

async function runDemo() {
  run.busy = true
  run.log = []
  try {
    for (const who of ['alice', 'bob', 'carol']) await ensureParticipant(who)
    say('carol never trades — watch the ladder.')

    const { closedDays } = await api.days()
    const today = new Date().toISOString().slice(0, 10)
    const last = closedDays[closedDays.length - 1]
    const dayA = last && last >= today ? nextDay(last) : today
    const dayB = nextDay(dayA)

    say(`— day ${dayA} —`)
    const a1 = await api.submitOrder({ participant: 'alice', symbol: 'AAPL', side: 'buy', qty: 100, limitPx: '10' })
    say(`submit  #${a1.clientOrderId} alice buy 100 AAPL @ 10`)
    await api.execute(a1.clientOrderId, 40, '10')
    say(`fill    #${a1.clientOrderId} → PARTIALLY_FILLED 40/100 (explicit)`)
    await api.cancelOrder(a1.clientOrderId)
    say(`cancel  #${a1.clientOrderId} → keeps the 40 that executed`)
    await beat()

    const a2 = await api.submitOrder({ participant: 'alice', symbol: 'MSFT', side: 'buy', qty: 50, limitPx: '20' })
    say(`submit  #${a2.clientOrderId} alice buy 50 MSFT @ 20`)
    await autoFill(a2.clientOrderId)

    const b1 = await api.submitOrder({ participant: 'bob', symbol: 'AAPL', side: 'buy', qty: 200, limitPx: '12' })
    say(`submit  #${b1.clientOrderId} bob buy 200 AAPL @ 12`)
    const rep = (await api.replaceOrder(b1.clientOrderId, 100, '11')) as { replacement: OrderView }
    say(`replace #${b1.clientOrderId} → #${rep.replacement.clientOrderId} for 100 @ 11`)
    await autoFill(rep.replacement.clientOrderId)

    await api.updateMarks([{ symbol: 'AAPL', px: '11' }, { symbol: 'MSFT', px: '21' }])
    say('marks   AAPL 11 · MSFT 21')
    await beat()
    await api.closeDay(dayA)
    say(`close   ${dayA} → leaderboard published`)
    await beat()

    say(`— day ${dayB} —`)
    const a3 = await api.submitOrder({ participant: 'alice', symbol: 'AAPL', side: 'sell', qty: 20, limitPx: '11' })
    say(`submit  #${a3.clientOrderId} alice sell 20 AAPL @ 11 — realizing profit`)
    await autoFill(a3.clientOrderId)

    await api.updateMarks([{ symbol: 'AAPL', px: '10.5' }, { symbol: 'MSFT', px: '22.25' }])
    say('marks   AAPL 10.5 · MSFT 22.25')
    await beat()
    await api.closeDay(dayB)
    say(`close   ${dayB} → ladder updated`)
    await beat()
    say('done — carol is listed on the ladder but unranked: never traded.')
  } catch (e) {
    say(e instanceof ApiError ? `stopped: ${e.status} ${e.detail}` : `stopped: ${e}`)
  } finally {
    run.busy = false
    await s.refresh()
  }
}
</script>

<template>
  <div class="sim">
    <section class="panel">
      <header>
        <h2>Simulation</h2>
        <button class="primary" :disabled="run.busy" @click="runDemo">
          {{ run.busy ? 'running…' : 'run two-day simulation' }}
        </button>
      </header>
      <div style="padding: 12px">
        <p class="dim" style="margin: 0 0 10px; font-size: 12.5px">
          The full two-day scenario, driven through the same public endpoints as
          the console — partial fill, a cancel that keeps its fills, a
          cancel-replace, marks moving, two day closes, and carol, who never
          trades. It adapts to live state: existing participants are reused and
          the next two unclosed days are used, so it can run again on top of
          itself. Watch the results fill in on the right as it goes.
        </p>
        <pre v-if="run.log.length" class="demolog"><code>{{ run.log.join('\n') }}</code></pre>
        <p v-else class="dim" style="font-size: 12.5px; margin: 0">
          Nothing has run yet — press the button.
        </p>
      </div>
    </section>
    <div class="results">
      <Portfolios />
      <OrdersPanel />
      <TapePanel />
      <Rankings />
    </div>
  </div>
</template>

<style scoped>
.sim {
  display: grid;
  grid-template-columns: minmax(320px, 5fr) 7fr;
  gap: 16px;
  align-items: start;
}
.sim > .panel { position: sticky; top: 16px; }
.results { display: grid; gap: 16px; min-width: 0; }
@media (max-width: 1000px) {
  .sim { grid-template-columns: 1fr; }
  .sim > .panel { position: static; }
}
.demolog {
  margin: 0;
  padding: 10px 12px;
  background: #0b1017;
  border: 1px solid var(--line);
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.6;
  max-height: 60vh;
  overflow-y: auto;
  white-space: pre-wrap;
}
</style>
