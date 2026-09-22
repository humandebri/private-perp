import { IDL } from '@icp-sdk/core/candid'
import type { SessionHandle } from './candid/funds_vault.did.js'

const Blob = IDL.Vec(IDL.Nat8)
const opt = <T>(type: IDL.Type<T>) => IDL.Opt(type)
const Session = IDL.Record({
  session_id: Blob,
  expires_at: IDL.Nat64,
  vault_principal: IDL.Principal,
  revocation_generation: IDL.Nat64,
})
const ActionState = IDL.Variant({
  Queued: IDL.Null,
  Signing: IDL.Null,
  Signed: IDL.Null,
  Dispatching: IDL.Null,
  Reconciled: IDL.Null,
  Unknown: IDL.Null,
  Aborted: IDL.Null,
})
const OrderState = IDL.Variant({
  Pending: IDL.Null,
  Open: IDL.Null,
  PartiallyFilled: IDL.Null,
  Filled: IDL.Null,
  Cancelled: IDL.Null,
  Rejected: IDL.Null,
  Unknown: IDL.Null,
})
const Side = IDL.Variant({ Buy: IDL.Null, Sell: IDL.Null })
const OrderKind = IDL.Variant({ LimitGtc: IDL.Null, MarketIoc: IDL.Null })
const Trigger = IDL.Record({
  kind: IDL.Variant({ TakeProfit: IDL.Null, StopLoss: IDL.Null }),
  is_market: IDL.Bool,
  trigger_price: IDL.Text,
})
const OrderView = IDL.Record({
  order_id: Blob,
  cloid: opt(Blob),
  market: IDL.Text,
  side: Side,
  kind: OrderKind,
  price: opt(IDL.Text),
  quantity: IDL.Text,
  filled_quantity: IDL.Text,
  state: OrderState,
  venue_state: opt(IDL.Text),
  hl_oid: opt(IDL.Nat64),
  cancel_requested: IDL.Bool,
  trigger: opt(Trigger),
  updated_at: IDL.Nat64,
})
const PendingOrder = IDL.Record({
  request_id: Blob,
  cloid: opt(Blob),
  order_id: opt(Blob),
  action_state: ActionState,
  since: IDL.Nat64,
  last_error: opt(IDL.Text),
})
const Position = IDL.Record({
  market: IDL.Text,
  size: IDL.Text,
  entry_price: IDL.Text,
  liquidation_price: opt(IDL.Text),
  unrealized_pnl: IDL.Int64,
  leverage: IDL.Nat32,
  margin_mode: IDL.Text,
  stop_loss: opt(IDL.Text),
  take_profit: opt(IDL.Text),
})
const Snapshot = IDL.Record({
  account_id: Blob,
  equity: IDL.Nat64,
  margin_used: IDL.Nat64,
  open_order_risk_reserved: IDL.Nat64,
  withdrawable: IDL.Nat64,
  unrealized_pnl: IDL.Int64,
  positions: IDL.Vec(Position),
  open_orders: IDL.Vec(OrderView),
  pending_orders: IDL.Vec(PendingOrder),
  observed_at: IDL.Nat64,
  revision: IDL.Nat64,
  data_age_ms: IDL.Nat64,
})
const OrderSummary = IDL.Record({
  order_id: Blob,
  cloid: Blob,
  market: IDL.Text,
  asset_index: IDL.Nat32,
  is_buy: IDL.Bool,
  kind: IDL.Text,
  price: opt(IDL.Text),
  quantity: IDL.Text,
  filled_quantity: IDL.Text,
  reduce_only: IDL.Bool,
  state: OrderState,
  dispatch_state: ActionState,
  preflight_state: ActionState,
  effective_leverage: IDL.Nat32,
  effective_slippage_bps: opt(IDL.Nat32),
  expires_after: opt(IDL.Nat64),
  last_error: opt(IDL.Text),
  cancel_requested: IDL.Bool,
  hl_oid: opt(IDL.Nat64),
  trigger: opt(Trigger),
  created_at: IDL.Nat64,
  updated_at: IDL.Nat64,
})
const Fill = IDL.Record({
  order_id: Blob,
  cloid: opt(Blob),
  market: IDL.Text,
  price: IDL.Text,
  quantity: IDL.Text,
  fee: IDL.Nat64,
  at: IDL.Nat64,
})
const page = <T>(item: IDL.Type<T>) =>
  IDL.Record({
    items: IDL.Vec(item),
    next_cursor: opt(Blob),
    revision: IDL.Nat64,
    observed_at: IDL.Nat64,
  })

export type Snapshot = {
  account_id: Uint8Array
  equity: bigint
  margin_used: bigint
  open_order_risk_reserved: bigint
  withdrawable: bigint
  unrealized_pnl: bigint
  positions: Position[]
  open_orders: OrderView[]
  pending_orders: PendingOrder[]
  observed_at: bigint
  revision: bigint
  data_age_ms: bigint
}
export type Position = {
  market: string
  size: string
  entry_price: string
  liquidation_price: [] | [string]
  unrealized_pnl: bigint
  leverage: number
  margin_mode: string
  stop_loss: [] | [string]
  take_profit: [] | [string]
}
export type OrderView = {
  order_id: Uint8Array
  market: string
  quantity: string
  filled_quantity: string
  state: Record<string, null>
  updated_at: bigint
  cancel_requested: boolean
}
export type PendingOrder = {
  request_id: Uint8Array
  order_id: [] | [Uint8Array]
  action_state: Record<string, null>
  since: bigint
  last_error: [] | [string]
}
export type OrderSummary = {
  created_at: bigint
  order_id: Uint8Array
  market: string
  is_buy: boolean
  kind: string
  price: [] | [string]
  quantity: string
  filled_quantity: string
  state: Record<string, null>
  dispatch_state: Record<string, null>
  preflight_state: Record<string, null>
  cancel_requested: boolean
  last_error: [] | [string]
  updated_at: bigint
  trigger: [] | [{ kind: Record<string, null>; is_market: boolean; trigger_price: string }]
  effective_leverage: number
  effective_slippage_bps: [] | [number]
}
export type Fill = { market: string; price: string; quantity: string; fee: bigint; at: bigint }
export type Page<T> = {
  items: T[]
  next_cursor: [] | [Uint8Array]
  revision: bigint
  observed_at: bigint
}

function encode(type: IDL.Type<unknown>, value: unknown): Uint8Array {
  return new Uint8Array(IDL.encode([type], [value]))
}
function decode<T>(type: IDL.Type<unknown>, value: Uint8Array): T {
  return IDL.decode([type], value)[0] as T
}
export const codec = {
  orderRequestQuery: (session: SessionHandle, client_request_id: Uint8Array) =>
    encode(IDL.Record({ session: Session, client_request_id: Blob }), {
      session,
      client_request_id,
    }),
  orderRequestStatus: (value: Uint8Array) =>
    decode<{ order: [] | [OrderSummary]; observed_at: bigint }>(
      IDL.Record({ order: opt(OrderSummary), observed_at: IDL.Nat64 }),
      value,
    ),
  snapshotQuery: (session: SessionHandle) => encode(IDL.Record({ session: Session }), { session }),
  listQuery: (session: SessionHandle, cursor?: Uint8Array) =>
    encode(IDL.Record({ session: Session, cursor: opt(Blob), limit: IDL.Nat32 }), {
      session,
      cursor: cursor ? [cursor] : [],
      limit: 100,
    }),
  cancelQuery: (session: SessionHandle, orderId: Uint8Array | number[]) =>
    encode(IDL.Record({ session: Session, order_id: Blob }), { session, order_id: orderId }),
  snapshot: (value: Uint8Array) => decode<Snapshot>(Snapshot, value),
  orders: (value: Uint8Array) => decode<Page<OrderSummary>>(page(OrderSummary), value),
  fills: (value: Uint8Array) => decode<Page<Fill>>(page(Fill), value),
  empty: (value: Uint8Array) => decode<null>(IDL.Null, value),
}
