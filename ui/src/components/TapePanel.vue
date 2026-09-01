<script setup lang="ts">
import { useCockpit } from '../store'
const s = useCockpit()
</script>

<template>
  <section class="panel">
    <header>
      <h2>Executions</h2>
      <span class="tag">{{ s.executions.length }} fills · the tape</span>
    </header>
    <table>
      <thead>
        <tr>
          <th class="num" title="the venue's id for this fill — FIX ExecID 17">tid</th>
          <th class="num" title="ours — FIX ClOrdID 11">cloid</th>
          <th class="num" title="the venue's order id — FIX OrderID 37">oid</th>
          <th>Participant</th>
          <th>Symbol</th>
          <th>Side</th>
          <th class="num">Qty</th>
          <th class="num">Px</th>
          <th class="num">Fee</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in [...s.executions].reverse()" :key="r.execId">
          <td class="num">{{ r.execId }}</td>
          <td class="num dim">{{ r.clientOrderId }}</td>
          <td class="num dim">{{ r.brokerOrderId ?? '—' }}</td>
          <td>{{ r.participant }}</td>
          <td>{{ r.symbol }}</td>
          <td :class="r.side === 'buy' ? 'up' : 'down'">{{ r.side }}</td>
          <td class="num">{{ r.qty }}</td>
          <td class="num">{{ r.px }}</td>
          <td class="num dim">{{ r.fee }}</td>
        </tr>
        <tr v-if="!s.executions.length">
          <td colspan="9" class="dim">No executions yet — the tape is empty.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
