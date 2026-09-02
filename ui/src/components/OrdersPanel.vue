<script setup lang="ts">
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
          <th class="num" title="clientOrderId — ours, FIX ClOrdID 11">cloid</th>
          <th class="num" title="brokerOrderId — the venue's, FIX OrderID 37">oid</th>
          <th>Participant</th>
          <th>Symbol</th>
          <th>Side</th>
          <th class="num">Qty</th>
          <th class="num">Limit</th>
          <th>State</th>
          <th class="num">Filled</th>
          <th class="num">Cost</th>
          <th class="num">Fees</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="o in [...s.orders].reverse()" :key="o.clientOrderId">
          <td class="num">{{ o.clientOrderId }}</td>
          <td class="num dim">{{ o.brokerOrderId ?? '—' }}</td>
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
          <td class="num dim">{{ o.fees }}</td>
        </tr>
        <tr v-if="!s.orders.length">
          <td colspan="11" class="dim">No orders yet.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
