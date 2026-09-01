// Typed client for the Rust API.
//
// The shapes here mirror `crates/api/src/dto.rs`. Money and returns arrive as
// decimal **strings** and are kept that way — parsing them into a JS `number`
// is the float mistake the whole backend exists to avoid, so they are only ever
// formatted for display, never arithmetic'd.

export interface OrderView {
  /** Ours — FIX ClOrdID (11). brokerOrderId is the venue's (37); fills carry execId (17). */
  clientOrderId: number
  participant: string
  symbol: string
  side: 'buy' | 'sell'
  qty: number
  limitPx: string
  state: string
  filledQty: number
  filledCost: string
  fees: string
  remainingQty: number
  brokerOrderId: number | null
  replaces: number | null
  submittedAt: number
}

export interface PositionView {
  symbol: string
  qty: number
  avgPrice: string | null
  costBasis: string
  mark: string | null
  marketValue: string | null
  unrealizedPnl: string | null
}

export interface PortfolioView {
  participant: string
  startingCash: string
  cash: string
  realizedPnl: string
  feesPaid: string
  unrealizedPnl: string | null
  totalValue: string | null
  valuationError: string | null
  positions: PositionView[]
  activeOrders: OrderView[]
}

export interface LeaderboardRowView {
  rank: number
  participant: string
  closingValue: string
  dailyPnl: string
  dailyReturn: string
  turnover: string
  active: boolean
  bust: boolean
}

export interface LeaderboardView {
  day: string
  rankedBy: string
  tiebreaks: string[]
  rows: LeaderboardRowView[]
}

export interface TapeRow {
  execId: number
  clientOrderId: number
  brokerOrderId: number | null
  participant: string
  symbol: string
  side: 'buy' | 'sell'
  qty: number
  px: string
  fee: string
}

export interface LadderRowView {
  rank: number | null
  participant: string
  cumulativeReturn: string
  dailyWins: number
  activeDays: number
  points: number
  eligible: boolean
}

export interface LadderView {
  rankedBy: string
  tiebreaks: string[]
  eligibility: string
  rows: LadderRowView[]
}

/** An RFC 7807 problem, surfaced with its extra members intact. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly type: string,
    readonly detail: string,
    readonly extra: Record<string, unknown>,
  ) {
    super(detail)
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

  const text = await res.text()
  const json = text ? JSON.parse(text) : null

  if (!res.ok) {
    const { type, detail, status, title, ...extra } = json ?? {}
    throw new ApiError(
      res.status,
      type ?? 'unknown',
      detail ?? title ?? `HTTP ${status ?? res.status}`,
      extra,
    )
  }
  return json as T
}

export interface InstrumentSpecView {
  symbol: string
  tick: string
  lot: number
  maxOrderQty: number | null
}

export const api = {
  health: () => request<Record<string, unknown>>('GET', '/health'),
  reset: () =>
    request<{ status: string; instruments: number }>('POST', '/reset'),
  instruments: () =>
    request<{ instruments: InstrumentSpecView[] }>('GET', '/instruments'),
  createInstrument: (symbol: string, tick: string, maxOrderQty?: number) =>
    request<InstrumentSpecView>('POST', '/instruments', {
      symbol,
      tick,
      ...(maxOrderQty !== undefined ? { maxOrderQty } : {}),
    }),
  removeInstrument: (symbol: string) => request('DELETE', `/instruments/${symbol}`),
  marks: () => request<{ marks: { symbol: string; px: string }[] }>('GET', '/market/prices'),
  participants: () => request<{ participants: string[] }>('GET', '/participants'),
  createParticipant: (id: string, startingCash: string) =>
    request('POST', '/participants', { id, startingCash }),
  portfolio: (id: string) => request<PortfolioView>('GET', `/participants/${id}/portfolio`),
  orders: () => request<{ orders: OrderView[] }>('GET', '/orders'),
  executions: () => request<{ executions: TapeRow[] }>('GET', '/executions'),
  submitOrder: (o: {
    participant: string
    symbol: string
    side: string
    qty: number
    limitPx: string
  }) => request<OrderView>('POST', '/orders', o),
  cancelOrder: (id: number) => request<OrderView>('DELETE', `/orders/${id}`),
  replaceOrder: (id: number, qty: number, limitPx: string) =>
    request('PUT', `/orders/${id}`, { qty, limitPx }),
  execute: (clientOrderId: number, qty?: number, px?: string) =>
    request<OrderView>('POST', '/broker/executions', {
      clientOrderId,
      ...(qty !== undefined && px !== undefined ? { qty, px } : {}),
    }),
  updateMarks: (marks: { symbol: string; px: string }[]) =>
    request('POST', '/market/prices', marks),
  closeDay: (day: string) => request<LeaderboardView>('POST', `/days/${day}/close`),
  days: () => request<{ closedDays: string[] }>('GET', '/days'),
  leaderboard: (day: string) => request<LeaderboardView>('GET', `/days/${day}/leaderboard`),
  ladder: () => request<LadderView>('GET', '/ladder'),
}

/**
 * `"0.002"` → `"+0.2000%"`, computed on the **string**, never through a float.
 *
 * The obvious implementation — `Number(d) * 100` then `.toFixed(4)` — disagrees
 * with the backend in the last digit: a cumulative return of `-0.0005055`
 * renders as `-0.0505%` in JS and `-0.0506%` in Rust, because the binary double
 * lands just below the midpoint. A frontend that rounds differently from the
 * service it displays is a frontend that will be trusted and be wrong.
 *
 * So: shift the point two places (×100) by moving digits, then round
 * half-away-from-zero with `BigInt` — matching `rust_decimal`'s `round_dp`.
 */
export function pct(decimal: string, dp = 4): string {
  const negative = decimal.startsWith('-')
  const [whole = '0', fraction = ''] = (negative ? decimal.slice(1) : decimal).split('.')

  // ×100: two fractional digits move into the integer part.
  const padded = fraction.padEnd(2, '0')
  const shiftedInt = (whole + padded.slice(0, 2)).replace(/^0+(?=\d)/, '')
  const digits = padded.slice(2).padEnd(dp + 1, '0')

  let scaled = BigInt(shiftedInt + digits.slice(0, dp))
  if (digits.charCodeAt(dp) - 48 >= 5) scaled += 1n

  const text = scaled.toString().padStart(dp + 1, '0')
  const sign = scaled === 0n ? '' : negative ? '-' : '+'
  return `${sign}${text.slice(0, text.length - dp)}.${text.slice(text.length - dp)}%`
}

/** Sign of a decimal string, without trusting float rounding near zero. */
export function signOf(decimal: string | null): 'up' | 'down' | 'flat' {
  if (decimal === null) return 'flat'
  if (/^-0*\.?0*$/.test(decimal) || /^-?0(\.0*)?$/.test(decimal)) return 'flat'
  return decimal.startsWith('-') ? 'down' : 'up'
}
