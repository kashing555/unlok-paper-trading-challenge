import { defineStore } from 'pinia'
import { api, ApiError, type LadderView, type LeaderboardView, type OrderView, type PortfolioView } from './api'

// One store, polled. The backend is the source of truth for everything shown
// here — nothing is derived locally and kept in step, for the same reason the
// backend does not do it either.
export const useCockpit = defineStore('cockpit', {
  state: () => ({
    connected: false,
    lastError: '' as string,
    participants: [] as string[],
    portfolios: {} as Record<string, PortfolioView>,
    orders: [] as OrderView[],
    days: [] as string[],
    board: null as LeaderboardView | null,
    selectedDay: '' as string,
    ladder: null as LadderView | null,
    events: 0,
  }),

  getters: {
    /** Symbols the competition currently has a position or an order in. */
    symbols(state): string[] {
      const set = new Set<string>()
      for (const o of state.orders) set.add(o.symbol)
      for (const p of Object.values(state.portfolios))
        for (const pos of p.positions) set.add(pos.symbol)
      return [...set].sort()
    },
    marks(state): Record<string, string> {
      const out: Record<string, string> = {}
      for (const p of Object.values(state.portfolios))
        for (const pos of p.positions) if (pos.mark) out[pos.symbol] = pos.mark
      return out
    },
  },

  actions: {
    async refresh() {
      try {
        const health = await api.health()
        this.connected = true
        this.events = Number(health.events ?? 0)

        const [{ participants }, { orders }, { closedDays }] = await Promise.all([
          api.participants(),
          api.orders(),
          api.days(),
        ])
        this.participants = participants
        this.orders = orders
        this.days = closedDays

        const books = await Promise.all(participants.map((p) => api.portfolio(p)))
        this.portfolios = Object.fromEntries(books.map((b) => [b.participant, b]))

        if (closedDays.length) {
          if (!this.selectedDay || !closedDays.includes(this.selectedDay))
            this.selectedDay = closedDays[closedDays.length - 1]!
          this.board = await api.leaderboard(this.selectedDay)
          this.ladder = await api.ladder()
        } else {
          this.board = null
          this.ladder = null
        }
      } catch (e) {
        this.connected = false
        this.lastError = e instanceof Error ? e.message : String(e)
      }
    },

    async selectDay(day: string) {
      this.selectedDay = day
      this.board = await api.leaderboard(day)
    },

    /** Runs an action, surfacing an RFC 7807 problem as readable text. */
    async attempt(fn: () => Promise<unknown>): Promise<boolean> {
      try {
        await fn()
        this.lastError = ''
        await this.refresh()
        return true
      } catch (e) {
        if (e instanceof ApiError) {
          const extra = Object.entries(e.extra)
            .map(([k, v]) => `${k}=${v}`)
            .join(' ')
          this.lastError = `${e.status} ${e.detail}${extra ? ` · ${extra}` : ''}`
        } else {
          this.lastError = e instanceof Error ? e.message : String(e)
        }
        return false
      }
    },
  },
})
