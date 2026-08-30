<script setup lang="ts">
import { signOf } from '../api'
import { useCockpit } from '../store'
const s = useCockpit()
</script>

<template>
  <section class="panel">
    <header>
      <h2>Portfolios</h2>
      <span class="tag">{{ s.participants.length }} participants</span>
    </header>
    <table>
      <thead>
        <tr>
          <th>Participant</th>
          <th class="num">Cash</th>
          <th class="num">Realized</th>
          <th class="num">Unrealized</th>
          <th class="num">Fees</th>
          <th class="num">Total value</th>
          <th>Positions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="p in s.participants" :key="p">
          <td>{{ p }}</td>
          <td class="num">{{ s.portfolios[p]?.cash ?? '—' }}</td>
          <td class="num" :class="signOf(s.portfolios[p]?.realizedPnl ?? null)">
            {{ s.portfolios[p]?.realizedPnl ?? '—' }}
          </td>
          <td class="num" :class="signOf(s.portfolios[p]?.unrealizedPnl ?? null)">
            {{ s.portfolios[p]?.unrealizedPnl ?? '—' }}
          </td>
          <td class="num dim">{{ s.portfolios[p]?.feesPaid ?? '—' }}</td>
          <td class="num">
            <template v-if="s.portfolios[p]?.totalValue">{{ s.portfolios[p]!.totalValue }}</template>
            <!-- Fail closed: a held symbol with no mark gets no number, and
                 says which symbol is missing rather than showing zero. -->
            <span v-else class="tag dead" :title="s.portfolios[p]?.valuationError ?? ''">no mark</span>
          </td>
          <td>
            <span v-if="!s.portfolios[p]?.positions.length" class="dim">flat</span>
            <span v-for="pos in s.portfolios[p]?.positions" :key="pos.symbol" class="tag" style="margin-right:4px">
              {{ pos.symbol }} {{ pos.qty }} @ {{ pos.avgPrice ?? '—' }}
            </span>
          </td>
        </tr>
        <tr v-if="!s.participants.length">
          <td colspan="7" class="dim">No participants yet — create one on the left.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
