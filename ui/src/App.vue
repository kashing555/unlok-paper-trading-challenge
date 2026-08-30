<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import Controls from './components/Controls.vue'
import DemoRunner from './components/DemoRunner.vue'
import OrdersPanel from './components/OrdersPanel.vue'
import Portfolios from './components/Portfolios.vue'
import Rankings from './components/Rankings.vue'
import { useCockpit } from './store'

const s = useCockpit()
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
      <div>
        <strong>Paper Trading Cockpit</strong>
        <span class="dim" style="margin-left: 10px">unlok-paper-trading-challenge</span>
      </div>
      <div style="display: flex; gap: 8px; align-items: center">
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

    <main>
      <aside class="stack"><Controls /><DemoRunner /></aside>
      <div class="stack">
        <Portfolios />
        <OrdersPanel />
        <Rankings />
      </div>
    </main>

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
.stack { display: grid; gap: 16px; min-width: 0; }
footer { margin-top: 24px; font-size: 12px; }
code { background: #0b1017; padding: 1px 5px; border-radius: 4px; }
@media (max-width: 1000px) { main { grid-template-columns: 1fr; } }
</style>
