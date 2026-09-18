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
}
export type FundEvent = { id: string; kind: string; amount: number; at: string }
export type DemoState = {
  active: boolean
  observedAt: number
  scenario: Scenario
  orders: DemoOrder[]
  reserve: number
  trading: number
  events: FundEvent[]
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
export const initialState = (): DemoState => ({
  active: false,
  observedAt: 0,
  scenario: 'open',
  orders: [],
  reserve: 0,
  trading: 0,
  events: [],
})

export function parseUsdc(value: string): number {
  if (!/^\d{1,9}(\.\d{1,6})?$/.test(value))
    throw new Error('USDCは正の数・小数6桁以内で入力してください。')
  const [whole, fraction = ''] = value.split('.')
  const units = Number(whole) * 1_000_000 + Number(fraction.padEnd(6, '0'))
  if (!Number.isSafeInteger(units) || units <= 0) throw new Error('金額が範囲外です。')
  return units
}
export function acceptOrder(
  state: DemoState,
  input: OrderInput,
  id: string,
  now: number,
): DemoState {
  if (!state.active) throw new Error('デモセッションを開始してください。')
  const fingerprint = JSON.stringify(input)
  const existing = state.orders.find((order) => order.id === id)
  if (existing) {
    if (existing.fingerprint !== fingerprint) throw new Error('同一IDの注文内容は変更できません。')
    return state
  }
  if (now - state.observedAt > 10_000)
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
    orders: [{ ...input, id, status: 'queued', filled: '0', fingerprint }, ...state.orders],
  }
}
export function settleOrder(state: DemoState, id: string, scenario: Scenario): DemoState {
  return {
    ...state,
    orders: state.orders.map((order) =>
      order.id !== id || order.status !== 'queued'
        ? order
        : {
            ...order,
            status:
              scenario === 'partial'
                ? 'partial'
                : scenario === 'unknown'
                  ? 'unknown'
                  : scenario === 'rejected'
                    ? 'rejected'
                    : order.kind === 'Market'
                      ? 'filled'
                      : 'open',
            filled:
              scenario === 'partial'
                ? String(Number(order.quantity) / 2)
                : scenario === 'open' && order.kind === 'Market'
                  ? order.quantity
                  : '0',
          },
    ),
  }
}
export function cancelOrder(state: DemoState, id: string, race = false): DemoState {
  return {
    ...state,
    orders: state.orders.map((order) =>
      order.id !== id || !['open', 'partial', 'queued'].includes(order.status)
        ? order
        : {
            ...order,
            status: race ? 'filled' : 'cancelled',
            filled: race ? order.quantity : order.filled,
          },
    ),
  }
}
export function moveFunds(
  state: DemoState,
  id: string,
  kind: 'deposit' | 'allocate' | 'recover' | 'withdraw',
  amount: number,
  at: string,
): DemoState {
  if (!state.active) throw new Error('デモセッションを開始してください。')
  const previous = state.events.find((event) => event.id === id)
  if (previous) {
    if (previous.kind !== kind || previous.amount !== amount)
      throw new Error('同一IDの資金要求は変更できません。')
    return state
  }
  if (!Number.isSafeInteger(amount) || amount <= 0) throw new Error('金額が範囲外です。')
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
    throw new Error('利用可能な合成残高が不足しています。')
  return { ...state, reserve, trading, events: [{ id, kind, amount, at }, ...state.events] }
}
