<script setup lang="ts">
import { reactive } from 'vue'
import { api } from '../api'
import { useCockpit } from '../store'

const s = useCockpit()

const create = reactive({ id: '', cash: '100000' })
const order = reactive({ participant: '', symbol: 'AAPL', side: 'buy', qty: 100, limitPx: '10' })
const mark = reactive({ symbol: 'AAPL', px: '10' })
const day = reactive({ value: new Date().toISOString().slice(0, 10) })
const inst = reactive({ symbol: '', tick: '0.01' })

async function addInstrument() {
  if (await s.attempt(() => api.createInstrument(inst.symbol, inst.tick))) inst.symbol = ''
}

async function addParticipant() {
  if (await s.attempt(() => api.createParticipant(create.id, create.cash))) create.id = ''
}

function submit() {
  return s.attempt(() =>
    api.submitOrder({
      participant: order.participant || s.participants[0] || '',
      symbol: order.symbol,
      side: order.side,
      qty: Number(order.qty),
      limitPx: order.limitPx,
    }),
  )
}
</script>

<template>
  <section class="panel">
    <header><h2>Actions</h2></header>
    <div style="padding: 12px; display: grid; gap: 16px">
      <div>
        <h3>New participant</h3>
        <div class="row">
          <input v-model="create.id" placeholder="id (e.g. alice)" style="flex:2" />
          <input v-model="create.cash" placeholder="starting cash" class="num" style="flex:1" />
          <button class="primary" :disabled="!create.id" @click="addParticipant">create</button>
        </div>
      </div>

      <div>
        <h3>Submit order</h3>
        <div class="row">
          <select v-model="order.participant" style="flex:1">
            <option value="">— participant —</option>
            <option v-for="p in s.participants" :key="p" :value="p">{{ p }}</option>
          </select>
          <select v-model="order.side"><option>buy</option><option>sell</option></select>
        </div>
        <div class="row">
          <input v-model="order.symbol" placeholder="SYMBOL" style="flex:1" />
          <input v-model.number="order.qty" type="number" min="1" class="num" style="flex:1" />
          <input v-model="order.limitPx" placeholder="limit" class="num" style="flex:1" />
          <button class="primary" :disabled="!s.participants.length" @click="submit">submit</button>
        </div>
        <p class="dim" style="margin:6px 0 0;font-size:12px">
          <template v-if="s.instruments.length">
            Tradable:
            <span
              v-for="i in s.instruments"
              :key="i.symbol"
              class="tag"
              style="margin-right:4px"
              :title="`tick ${i.tick} · lot ${i.lot}` + (i.maxOrderQty ? ` · max ${i.maxOrderQty}` : '')"
            >{{ i.symbol }} <span style="opacity:.6">{{ i.tick }}</span></span>
            — off-list or off-tick orders come back REJECTED.
          </template>
          <template v-else>
            Any upper-case symbol is tradable (tick 0.0001) — lower-case is
            rejected, not corrected: two spellings of one key file executions
            twice.
          </template>
        </p>
      </div>

      <div>
        <h3>Instruments</h3>
        <p v-if="s.instruments.length" style="margin:0 0 6px">
          <span v-for="i in s.instruments" :key="i.symbol" class="tag" style="margin-right:4px">
            {{ i.symbol }}
            <a href="#" style="margin-left:2px" title="delist" @click.prevent="s.attempt(() => api.removeInstrument(i.symbol))">×</a>
          </span>
        </p>
        <div class="row">
          <input v-model="inst.symbol" placeholder="SYMBOL" style="flex:1" />
          <input v-model="inst.tick" placeholder="tick e.g. 0.01" class="num" style="flex:1" />
          <button :disabled="!inst.symbol || !inst.tick" @click="addInstrument">list</button>
        </div>
        <p class="dim" style="margin:6px 0 0;font-size:12px">
          The security master. Empty = unrestricted; the first listing switches
          the venue to allowlist mode. Delisting is refused while orders or
          positions reference the symbol.
        </p>
      </div>

      <div>
        <h3>Update mark</h3>
        <p v-if="s.marks.length" class="dim" style="margin:0 0 6px;font-size:12px">
          Current:
          <span v-for="m in s.marks" :key="m.symbol" class="tag" style="margin-right:4px">
            {{ m.symbol }} {{ m.px }}
          </span>
        </p>
        <div class="row">
          <input v-model="mark.symbol" placeholder="SYMBOL" style="flex:1" />
          <input v-model="mark.px" placeholder="price" class="num" style="flex:1" />
          <button @click="s.attempt(() => api.updateMarks([{ symbol: mark.symbol, px: mark.px }]))">set</button>
        </div>
      </div>

      <div>
        <h3>Close trading day</h3>
        <div class="row">
          <input v-model="day.value" type="date" style="flex:1" />
          <button class="primary" @click="s.attempt(() => api.closeDay(day.value))">close</button>
        </div>
        <p class="dim" style="margin:6px 0 0;font-size:12px">
          Idempotent: re-closing a day returns the published board rather than
          recomputing it. Fails if a held symbol has no mark.
        </p>
      </div>
    </div>
  </section>
</template>

<style scoped>
h3 {
  margin: 0 0 6px;
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--dim);
}
.row { display: flex; gap: 6px; margin-bottom: 6px; }
.row input, .row select { min-width: 0; }
</style>
