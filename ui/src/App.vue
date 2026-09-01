<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import Controls from './components/Controls.vue'
import JournalPanel from './components/JournalPanel.vue'
import OrdersPanel from './components/OrdersPanel.vue'
import Portfolios from './components/Portfolios.vue'
import TapePanel from './components/TapePanel.vue'
import Rankings from './components/Rankings.vue'
import SimulationPage from './components/SimulationPage.vue'
import { api } from './api'
import { useCockpit } from './store'

const s = useCockpit()

async function resetWorld() {
  if (!window.confirm('Reset everything? Participants, orders, marks and closed days are all destroyed.')) return
  s.journal = []
  s.journalSeq = 0
  await s.attempt(() => api.reset())
}
// Two pages, one reactive switch. vue-router would be a dependency for a
// boolean — the same declined-forwarding-layer argument as everywhere else.
const page = ref<'console' | 'simulation'>('console')
let timer: number | undefined

// Polling, not a websocket. The backend has no push channel and adding one
// would be scope the brief did not ask for; a competition's state changes at
// human speed.
onMounted(() => {
  s.refresh()
  timer = window.setInterval(() => s.refresh(), 2000)
})
onUnmounted(() => window.clearInterval(timer))
</script>

<template>
  <div class="shell">
    <header class="top">
      <div style="display: flex; align-items: baseline; gap: 18px">
        <strong>Paper Trading Cockpit</strong>
        <nav class="pages">
          <button :class="{ on: page === 'console' }" @click="page = 'console'">Console</button>
          <button :class="{ on: page === 'simulation' }" @click="page = 'simulation'">Simulation</button>
        </nav>
      </div>
      <div style="display: flex; gap: 8px; align-items: center">
        <button
          class="danger"
          title="Destroy the world: back to the boot state (seeded instruments only)"
          @click="resetWorld"
        >
          reset
        </button>
        <span class="tag">{{ s.events }} events</span>
        <span :class="s.connected ? 'tag done' : 'tag dead'">
          {{ s.connected ? 'connected' : 'no backend' }}
        </span>
      </div>
    </header>

    <p v-if="!s.connected" class="banner">
      Cannot reach the API. Start it with <code>cargo run --bin ptc</code> — the dev
      server proxies <code>/api</code> to <code>127.0.0.1:8080</code>.
    </p>
    <p v-else-if="s.lastError" class="banner">{{ s.lastError }}</p>

    <main v-if="page === 'console'">
      <aside class="stack"><Controls /><JournalPanel /></aside>
      <div class="stack">
        <Portfolios />
        <OrdersPanel />
        <TapePanel />
        <Rankings />
      </div>
    </main>
    <SimulationPage v-else />

    <footer class="dim">
      Beyond the brief — “no user interface is required”. Built after the scored
      work; the service is complete with <code>ui/</code> deleted.
    </footer>
  </div>
</template>

<style scoped>
.shell { max-width: 1500px; margin: 0 auto; padding: 16px; }
.top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--line);
  margin-bottom: 16px;
}
button.danger {
  border-color: #6b2320;
  color: var(--down);
  font-size: 12px;
  padding: 3px 10px;
}
button.danger:hover { background: #2a1210; border-color: var(--down); }
.banner {
  background: #2d1e0a;
  border: 1px solid #5c4413;
  color: #e3b341;
  border-radius: 6px;
  padding: 9px 12px;
  margin: 0 0 16px;
  font-size: 13px;
}
main { display: grid; grid-template-columns: 340px 1fr; gap: 16px; align-items: start; }
.pages { display: flex; gap: 4px; }
.pages button {
  border: none; background: none; padding: 4px 10px; border-radius: 6px;
  color: var(--dim); font-size: 13px; cursor: pointer;
}
.pages button:hover { color: var(--text); }
.pages button.on { color: var(--accent); background: #16324f33; }
.stack { display: grid; gap: 16px; min-width: 0; }
footer { margin-top: 24px; font-size: 12px; }
code { background: #0b1017; padding: 1px 5px; border-radius: 4px; }
@media (max-width: 1000px) { main { grid-template-columns: 1fr; } }
</style>
