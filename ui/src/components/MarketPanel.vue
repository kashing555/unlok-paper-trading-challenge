<script setup lang="ts">
import { useCockpit } from '../store'

const s = useCockpit()
</script>

<template>
  <section class="panel">
    <header><h2>Market</h2></header>
    <div style="padding: 12px; display: grid; gap: 10px; font-size: 13px">
      <p v-if="s.instruments.length" style="margin: 0">
        <span class="dim">Tradable:</span>
        <span
          v-for="i in s.instruments"
          :key="i.symbol"
          class="tag"
          style="margin-left: 4px"
          :title="`tick ${i.tick} · lot ${i.lot}` + (i.maxOrderQty ? ` · max ${i.maxOrderQty}` : '')"
        >{{ i.symbol }} <span style="opacity: 0.6">{{ i.tick }}</span></span>
      </p>
      <p v-else class="dim" style="margin: 0">
        Empty security master — any upper-case symbol is tradable (tick 0.0001).
      </p>
      <p v-if="s.marks.length" style="margin: 0">
        <span class="dim">Marks:</span>
        <span v-for="m in s.marks" :key="m.symbol" class="tag" style="margin-left: 4px">
          {{ m.symbol }} {{ m.px }}
        </span>
      </p>
      <p v-else class="dim" style="margin: 0">No marks posted yet.</p>
    </div>
  </section>
</template>
