<script setup lang="ts">
import { api } from '../api'
import { useCockpit } from '../store'

const s = useCockpit()

const TERMINAL = ['FILLED', 'CANCELLED', 'REJECTED']
const isWorking = (state: string) => !TERMINAL.includes(state)

function stateClass(state: string) {
  if (state === 'FILLED') return 'tag done'
  if (state === 'CANCELLED' || state === 'REJECTED') return 'tag dead'
  return 'tag live'
}
</script>

<template>
  <section class="panel">
    <header>
      <h2>Orders</h2>
      <span class="tag">{{ s.orders.filter((o) => isWorking(o.state)).length }} working</span>
    </header>
    <table>
      <thead>
        <tr>
          <th class="num">#</th>
          <th>Participant</th>
          <th>Symbol</th>
          <th>Side</th>
          <th class="num">Qty</th>
          <th class="num">Limit</th>
          <th>State</th>
          <th class="num">Filled</th>
          <th class="num">Cost</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="o in [...s.orders].reverse()" :key="o.id">
          <td class="num">{{ o.id }}</td>
          <td>{{ o.participant }}</td>
          <td>{{ o.symbol }}</td>
          <td :class="o.side === 'buy' ? 'up' : 'down'">{{ o.side }}</td>
          <td class="num">{{ o.qty }}</td>
          <td class="num">{{ o.limitPx }}</td>
          <td>
            <span :class="stateClass(o.state)">{{ o.state }}</span>
            <!-- The cancel-replace chain, so a reader can see where the
                 filled quantity of a replaced order went. -->
            <span v-if="o.replaces" class="dim" style="margin-left:6px">← #{{ o.replaces }}</span>
          </td>
          <td class="num">{{ o.filledQty }}/{{ o.qty }}</td>
          <td class="num">{{ o.filledCost }}</td>
          <td style="white-space:nowrap">
            <button v-if="isWorking(o.state)" @click="s.attempt(() => api.execute(o.id))" title="Let the seeded broker choose the terms">
              fill
            </button>
            <button v-if="isWorking(o.state)" @click="s.attempt(() => api.cancelOrder(o.id))">cancel</button>
          </td>
        </tr>
        <tr v-if="!s.orders.length">
          <td colspan="10" class="dim">No orders yet.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
