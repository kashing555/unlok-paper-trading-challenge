<script setup lang="ts">
import { reactive } from 'vue'
import { api, ApiError, type OrderView } from '../api'
import { useCockpit } from '../store'

const s = useCockpit()

// The ptc-demo scenario, driven through the same REST endpoints as every
// button in this cockpit — no private path, so what you watch is exactly
// what the API can do. Adapts to live state: participants are created only
// if missing, and the two closed days are the next ones after whatever is
// already closed, so it can run on a fresh server or on top of manual play.
const run = reactive({ busy: false, log: [] as string[] })

const say = (m: string) => run.log.push(m)
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
    say(`fill    #${id} → ${last.state} ${last.filledQty}/${last.qty}`)
    if (TERMINAL.includes(last.state)) return last
  }
  throw new Error(`order #${id} did not terminate`)
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
    say(`submit  #${a1.id} alice buy 100 AAPL @ 10`)
    await api.execute(a1.id, 40, '10')
    say(`fill    #${a1.id} → PARTIALLY_FILLED 40/100 (explicit)`)
    await api.cancelOrder(a1.id)
    say(`cancel  #${a1.id} → keeps the 40 that executed`)

    const a2 = await api.submitOrder({ participant: 'alice', symbol: 'MSFT', side: 'buy', qty: 50, limitPx: '20' })
    say(`submit  #${a2.id} alice buy 50 MSFT @ 20`)
    await autoFill(a2.id)

    const b1 = await api.submitOrder({ participant: 'bob', symbol: 'AAPL', side: 'buy', qty: 200, limitPx: '12' })
    say(`submit  #${b1.id} bob buy 200 AAPL @ 12`)
    const rep = (await api.replaceOrder(b1.id, 100, '11')) as { replacement: OrderView }
    say(`replace #${b1.id} → #${rep.replacement.id} for 100 @ 11`)
    await autoFill(rep.replacement.id)

    await api.updateMarks([{ symbol: 'AAPL', px: '11' }, { symbol: 'MSFT', px: '21' }])
    say('marks   AAPL 11 · MSFT 21')
    await api.closeDay(dayA)
    say(`close   ${dayA} → leaderboard published`)

    say(`— day ${dayB} —`)
    const a3 = await api.submitOrder({ participant: 'alice', symbol: 'AAPL', side: 'sell', qty: 20, limitPx: '11' })
    say(`submit  #${a3.id} alice sell 20 AAPL @ 11 — realizing profit`)
    await autoFill(a3.id)

    await api.updateMarks([{ symbol: 'AAPL', px: '10.5' }, { symbol: 'MSFT', px: '22.25' }])
    say('marks   AAPL 10.5 · MSFT 22.25')
    await api.closeDay(dayB)
    say(`close   ${dayB} → ladder updated`)
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
  <section class="panel">
    <header>
      <h2>Scripted demo</h2>
      <button class="primary" :disabled="run.busy" @click="runDemo">
        {{ run.busy ? 'running…' : 'run two-day demo' }}
      </button>
    </header>
    <div style="padding: 10px 12px">
      <p class="dim" style="margin: 0 0 8px; font-size: 12px">
        Drives the full scenario through the same endpoints as the buttons
        above: partial fill, cancel keeping its fills, cancel-replace, marks,
        two day closes, and a participant who never trades.
      </p>
      <pre v-if="run.log.length" class="demolog"><code>{{ run.log.join('\n') }}</code></pre>
    </div>
  </section>
</template>

<style scoped>
.demolog {
  margin: 0;
  padding: 10px 12px;
  background: #0b1017;
  border: 1px solid var(--line);
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.6;
  max-height: 260px;
  overflow-y: auto;
  white-space: pre-wrap;
}
</style>
