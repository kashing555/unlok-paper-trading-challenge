<script setup lang="ts">
import { pct, signOf } from '../api'
import { useCockpit } from '../store'
const s = useCockpit()
</script>

<template>
  <section class="panel">
    <header>
      <h2>Daily leaderboard</h2>
      <select v-if="s.days.length" :value="s.selectedDay" @change="s.selectDay(($event.target as HTMLSelectElement).value)">
        <option v-for="d in s.days" :key="d" :value="d">{{ d }}</option>
      </select>
    </header>

    <p v-if="!s.board" class="dim" style="padding:12px">
      No day closed yet. Close one to publish a leaderboard.
    </p>

    <template v-else>
      <!-- The API states its own ranking rules; showing them means a reader
           never has to infer why one row is above another. -->
      <p class="dim" style="padding:8px 12px;margin:0;font-size:12px">
        {{ s.board.rankedBy }} · then {{ s.board.tiebreaks.join(' · then ') }}
      </p>
      <table>
        <thead>
          <tr>
            <th class="num">#</th>
            <th>Participant</th>
            <th class="num">Closing value</th>
            <th class="num">Daily P&amp;L</th>
            <th class="num">Return</th>
            <th class="num">Turnover</th>
            <th>Active</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in s.board.rows" :key="r.participant">
            <td class="num">{{ r.rank }}</td>
            <td>{{ r.participant }}</td>
            <td class="num">{{ r.closingValue }}</td>
            <td class="num" :class="signOf(r.dailyPnl)">{{ r.dailyPnl }}</td>
            <td class="num" :class="signOf(r.dailyReturn)">{{ pct(r.dailyReturn) }}</td>
            <td class="num">{{ r.turnover }}</td>
            <td><span :class="r.active ? 'tag done' : 'tag'">{{ r.active ? 'yes' : 'no' }}</span></td>
          </tr>
        </tbody>
      </table>
    </template>
  </section>

  <section class="panel">
    <header>
      <h2>Overall ladder</h2>
      <span class="tag">{{ s.days.length }} days closed</span>
    </header>

    <p v-if="!s.ladder" class="dim" style="padding:12px">Nothing to rank yet.</p>

    <template v-else>
      <p class="dim" style="padding:8px 12px;margin:0;font-size:12px">
        {{ s.ladder.rankedBy }}<br />{{ s.ladder.eligibility }}
      </p>
      <table>
        <thead>
          <tr>
            <th class="num">#</th>
            <th>Participant</th>
            <th class="num">Cumulative</th>
            <th class="num">Wins</th>
            <th class="num">Active days</th>
            <th class="num">Points</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in s.ladder.rows" :key="r.participant">
            <td class="num">
              <template v-if="r.rank">{{ r.rank }}</template>
              <span v-else class="dim" title="Never traded — listed, but not placed">—</span>
            </td>
            <td>
              {{ r.participant }}
              <span v-if="!r.eligible" class="tag" title="Ladder placement needs at least one active day">never traded</span>
            </td>
            <td class="num" :class="signOf(r.cumulativeReturn)">{{ pct(r.cumulativeReturn) }}</td>
            <td class="num">{{ r.dailyWins }}</td>
            <td class="num">{{ r.activeDays }}</td>
            <td class="num dim">{{ r.points }}</td>
          </tr>
        </tbody>
      </table>
    </template>
  </section>
</template>
