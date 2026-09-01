<script setup lang="ts">
import { useCockpit } from '../store'

const s = useCockpit()

function clock(ms: number): string {
  return new Date(ms).toTimeString().slice(0, 8)
}
</script>

<template>
  <section class="panel">
    <header>
      <h2>Journal</h2>
      <span class="tag">seq {{ s.journalSeq }}</span>
    </header>
    <div class="feed">
      <p v-if="!s.journal.length" class="dim" style="padding: 10px 12px; margin: 0; font-size: 12.5px">
        The event log, narrated. Act anywhere — Swagger, curl, this page — and
        it appears here within two seconds. Newest first.
      </p>
      <div v-for="e in s.journal" :key="e.seq" class="row">
        <span class="dim mono">{{ e.seq }}</span>
        <span class="dim mono">{{ clock(e.at) }}</span>
        <span class="mono">{{ e.summary }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.feed {
  max-height: 340px;
  overflow-y: auto;
  padding: 6px 0;
}
.row {
  display: grid;
  grid-template-columns: 34px 62px 1fr;
  gap: 8px;
  padding: 3px 12px;
  font-size: 12px;
  line-height: 1.5;
  border-bottom: 1px solid #1a212c;
}
.row:last-child { border-bottom: none; }
.mono {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
