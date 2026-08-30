<script setup lang="ts">
import { reactive } from 'vue'
import { api } from '../api'
import { useCockpit } from '../store'

const s = useCockpit()

const TERMINAL = ['FILLED', 'CANCELLED', 'REJECTED']
const isWorking = (state: string) => !TERMINAL.includes(state)

// Cancel-replace, per order: opening the editor pre-fills the order's own
// remaining terms; confirming mints a new order linked via `replaces`.
const edit = reactive({ id: null as number | null, qty: 0, px: '' })

function openReplace(id: number, qty: number, px: string) {
  edit.id = id
  edit.qty = qty
  edit.px = px
}

async function confirmReplace() {
  if (edit.id === null) return
  const ok = await s.attempt(() => api.replaceOrder(edit.id!, Number(edit.qty), edit.px))
  if (ok) edit.id = null
}

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
          <th class="num">Fees</th>
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
          <td class="num dim">{{ o.fees }}</td>
          <td style="white-space:nowrap">
            <template v-if="edit.id === o.id">
              <input v-model.number="edit.qty" type="number" min="1" class="num" style="width:70px" title="new quantity" />
              <input v-model="edit.px" class="num" style="width:80px" title="new limit price" />
              <button class="primary" @click="confirmReplace">ok</button>
              <button @click="edit.id = null">×</button>
            </template>
            <template v-else-if="isWorking(o.state)">
              <button @click="s.attempt(() => api.execute(o.id))" title="Let the seeded broker choose the terms">fill</button>
              <button @click="openReplace(o.id, o.remainingQty, o.limitPx)" title="Cancel-replace: keeps what already filled, mints a new order">replace</button>
              <button @click="s.attempt(() => api.cancelOrder(o.id))">cancel</button>
            </template>
          </td>
        </tr>
        <tr v-if="!s.orders.length">
          <td colspan="11" class="dim">No orders yet.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
