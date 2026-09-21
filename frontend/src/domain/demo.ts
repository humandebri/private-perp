export type Scenario = 'open' | 'partial' | 'unknown' | 'rejected' | 'cancel-race'
export type OrderStatus =
  | 'queued'
  | 'open'
  | 'partial'
  | 'filled'
  | 'cancelled'
  | 'rejected'
  | 'unknown'
export type OrderInput = {
  market: 'BTC' | 'ETH'
  side: 'buy' | 'sell'
  kind: 'Market' | 'Limit'
  quantity: string
  price: string
}
export type DemoOrder = OrderInput & {
  id: string
  status: OrderStatus
  filled: string
  fingerprint: string
  /** Snapshot of the answer scenario at dispatch time; the panel may change later. */
  scenario: Scenario
}
export type FundKind = 'deposit' | 'allocate' | 'recover' | 'withdraw'
export type FundEvent = { id: string; kind: string; amount: number; at: string }
/** Every fund request is remembered with its outcome, accepted or rejected. */
export type FundRequest = { id: string; kind: FundKind; amount: number; error: string | null }
export type DemoState = {
  active: boolean
  observedAt: number
  scenario: Scenario
  orders: DemoOrder[]
  reserve: number
  trading: number
  events: FundEvent[]
  fundRequests: FundRequest[]
}
export const labels: Record<OrderStatus, string> = {
  queued: '受付済み・送信準備中',
  open: 'HL受理（模擬）',
  partial: '部分約定（模擬）',
  filled: '約定（模擬）',
  cancelled: '取消済み（模擬）',
  rejected: '拒否（模擬）',
  unknown: '結果不明・再送しない',
}
/** Account data older than this cannot be used for new orders. */
export const STALE_AFTER_MS = 10_000
/** Shared copy so the notice and the threshold cannot drift apart. */
export const STALE_NOTICE = `${STALE_AFTER_MS / 1000}秒以上前 · 新規注文停止`
/** How often the UI re-reads account freshness in this browser tab. */
export const STALE_SAMPLE_INTERVAL_MS = 2_000
/** Synthetic quantity precision (1e-5). */
export const QUANTITY_DECIMALS = 5
export const initialState = (): DemoState => ({
  active: false,
  observedAt: 0,
  scenario: 'open',
  orders: [],
  reserve: 0,
  trading: 0,
  events: [],
  fundRequests: [],
})

export function parseUsdc(value: string): number {
  if (!/^\d{1,9}(\.\d{1,6})?$/.test(value))
    throw new Error('USDCは正の数・小数6桁以内で入力してください。')
  const [whole, fraction = ''] = value.split('.')
  const units = Number(whole) * 1_000_000 + Number(fraction.padEnd(6, '0'))
  if (!Number.isSafeInteger(units) || units <= 0) throw new Error('金額が範囲外です。')
  return units
}
/** Reads a plain decimal string into exact integer units; never float arithmetic. */
export function toScaledInteger(value: string, decimals: number): number {
  if (!/^\d+(\.\d+)?$/.test(value)) throw new Error('数量は十進数で入力してください。')
  const [whole, fraction = ''] = value.split('.')
  if (fraction.length > decimals) throw new Error('数量の桁数が多すぎます。')
  return Number(whole) * 10 ** decimals + Number(fraction.padEnd(decimals, '0'))
}
/** Renders exact integer units back to a decimal string without trailing zeros. */
export function fromScaledInteger(units: number, decimals: number): string {
  if (!Number.isSafeInteger(units) || units < 0) throw new Error('数量が範囲外です。')
  const base = 10 ** decimals
  const whole = Math.floor(units / base)
  const fraction = String(units % base)
    .padStart(decimals, '0')
    .replace(/0+$/, '')
  return fraction ? `${whole}.${fraction}` : String(whole)
}
/**
 * Exact half of a quantity, split into the fill and the untouched remainder.
 * Integer units keep both parts inside the quantity precision limit, so a
 * partial fill can never round into `0.000005`.
 */
export function halveQuantity(
  value: string,
  decimals = QUANTITY_DECIMALS,
): { filled: string; remainder: string } {
  const units = toScaledInteger(value, decimals)
  // Explicit quotient and remainder in integer units: floor division never
  // rounds the fill, and the leftover stays a whole number of units.
  const quotient = Math.floor(units / 2)
  const remainder = units - quotient
  if (quotient < 0 || remainder < 0) throw new Error('数量が範囲外です。')
  return {
    filled: fromScaledInteger(quotient, decimals),
    remainder: fromScaledInteger(remainder, decimals),
  }
}
/** Key order must not change the identity of a request: hash explicit fields. */
export function orderFingerprint(input: OrderInput): string {
  return JSON.stringify([input.market, input.side, input.kind, input.quantity, input.price])
}
export function acceptOrder(
  state: DemoState,
  input: OrderInput,
  id: string,
  now: number,
): DemoState {
  if (!state.active) throw new Error('デモセッションを開始してください。')
  const fingerprint = orderFingerprint(input)
  const existing = state.orders.find((order) => order.id === id)
  if (existing) {
    if (existing.fingerprint !== fingerprint) throw new Error('同一IDの注文内容は変更できません。')
    return state
  }
  if (now - state.observedAt > STALE_AFTER_MS)
    throw new Error('口座データが古いため新規注文を停止しています。')
  if (
    !/^\d+(\.\d{1,5})?$/.test(input.quantity) ||
    Number(input.quantity) <= 0 ||
    Number(input.quantity) > 100
  )
    throw new Error('数量は0より大きく100以下、小数5桁以内で入力してください。')
  if (
    !/^\d+(\.\d{1,2})?$/.test(input.price) ||
    Number(input.price) <= 0 ||
    Number(input.price) > 1_000_000
  )
    throw new Error('価格が範囲外です。')
  if (state.orders.length >= 100) throw new Error('デモ注文は100件までです。')
  return {
    ...state,
    orders: [
      { ...input, id, status: 'queued', filled: '0', fingerprint, scenario: state.scenario },
      ...state.orders,
    ],
  }
}
export function settleOrder(state: DemoState, id: string, scenario: Scenario): DemoState {
  return {
    ...state,
    orders: state.orders.map((order) => {
      if (order.id !== id || order.status !== 'queued') return order
      if (scenario === 'partial') {
        const { filled } = halveQuantity(order.quantity)
        // Below two units of 1e-5 there is no representable half: leave the order
        // unfilled instead of rounding the fill past the 5-decimal limit.
        return filled === '0'
          ? { ...order, status: 'open', filled: '0' }
          : { ...order, status: 'partial', filled }
      }
      const market = order.kind === 'Market'
      return {
        ...order,
        status:
          scenario === 'unknown'
            ? 'unknown'
            : scenario === 'rejected'
              ? 'rejected'
              : market
                ? 'filled'
                : 'open',
        filled: market ? order.quantity : '0',
      }
    }),
  }
}
export function cancelOrder(state: DemoState, id: string): DemoState {
  return {
    ...state,
    orders: state.orders.map((order) => {
      if (order.id !== id) return order
      // Never dispatched: the only truthful outcome is cancelled, whatever the
      // panel scenario says, because no fill can have happened yet.
      if (order.status === 'queued') return { ...order, status: 'cancelled' }
      if (!['open', 'partial'].includes(order.status)) return order
      return order.scenario === 'cancel-race'
        ? { ...order, status: 'filled', filled: order.quantity }
        : { ...order, status: 'cancelled' }
    }),
  }
}
export type FundMoveResult = { state: DemoState; error: string | null }
export function moveFunds(
  state: DemoState,
  id: string,
  kind: FundKind,
  amount: number,
  at: string,
): FundMoveResult {
  if (!state.active) throw new Error('デモセッションを開始してください。')
  const previous = state.fundRequests.find((request) => request.id === id)
  if (previous) {
    // The id is spent for good: a retry must repeat the recorded body, and the
    // recorded outcome is returned again instead of being applied twice.
    if (previous.kind !== kind || previous.amount !== amount)
      return { state, error: '同一IDの資金要求は変更できません。' }
    return { state, error: previous.error }
  }
  const record = (error: string | null): FundMoveResult => ({
    state: {
      ...state,
      fundRequests: [{ id, kind, amount, error }, ...state.fundRequests],
    },
    error,
  })
  if (!Number.isSafeInteger(amount) || amount <= 0) return record('金額が範囲外です。')
  let { reserve, trading } = state
  if (kind === 'deposit') reserve += amount
  if (kind === 'allocate') {
    reserve -= amount
    trading += amount
  }
  if (kind === 'recover') {
    trading -= amount
    reserve += amount
  }
  if (kind === 'withdraw') reserve -= amount
  if (
    reserve < 0 ||
    trading < 0 ||
    !Number.isSafeInteger(reserve) ||
    !Number.isSafeInteger(trading)
  )
    return record('利用可能な合成残高が不足しています。')
  const accepted = record(null)
  return {
    ...accepted,
    state: {
      ...accepted.state,
      reserve,
      trading,
      events: [{ id, kind, amount, at }, ...accepted.state.events],
    },
  }
}
