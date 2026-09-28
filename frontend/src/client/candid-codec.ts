import { IDL } from '@icp-sdk/core/candid'
import type { SessionHandle } from './candid/funds_vault.did.js'
import type {
  AgentGeneration as AgentGenerationView,
  FundRequestState,
} from './candid/funds_vault.did.js'
import { unwrap } from './result'

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
  fee: IDL.Int64,
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

const NotAllowedCode = IDL.Variant({
  UpgradeContentMismatch: IDL.Null,
  AssetNotAllowed: IDL.Null,
  SessionIssuedByUnregisteredVault: IDL.Null,
  UpgradeTooEarly: IDL.Null,
  AccountNotOwned: IDL.Null,
  OrderNotFound: IDL.Null,
  CallerMismatch: IDL.Null,
  OrderNotCancellable: IDL.Null,
  UpgradeAlreadyExecuted: IDL.Null,
  OperationNotAvailable: IDL.Null,
  UpgradeNotScheduled: IDL.Null,
})
const BadRequestCode = IDL.Variant({
  NonceReused: IDL.Null,
  UnsupportedMarket: IDL.Null,
  TooLarge: IDL.Null,
  NetworkMismatch: IDL.Null,
  MalformedPayload: IDL.Null,
  InvalidSignature: IDL.Null,
  QuantityOutOfRange: IDL.Null,
  ChallengeExpired: IDL.Null,
  ExpiredIntent: IDL.Null,
  AmountZero: IDL.Null,
  PrecisionExceeded: IDL.Null,
  OriginMismatch: IDL.Null,
  ChallengeReused: IDL.Null,
  DestinationNotAllowed: IDL.Null,
  MissingField: IDL.Null,
  PriceOutOfRange: IDL.Null,
  UnsupportedAsset: IDL.Null,
})
const ErrorCode = IDL.Variant({
  Internal: IDL.Record({ code: IDL.Text }),
  DuplicateIgnored: IDL.Record({ request_id: Blob }),
  SigningQueueFull: IDL.Null,
  NotAllowed: IDL.Record({ code: NotAllowedCode }),
  UpstreamUnavailable: IDL.Record({ venue: IDL.Text }),
  UnknownPending: IDL.Record({ action_id: Blob }),
  ReservationConflict: IDL.Null,
  StaleAccountState: IDL.Record({ max_age_ms: IDL.Nat64, observed_at: IDL.Nat64 }),
  IdempotencyConflict: IDL.Record({ request_id: Blob }),
  UpstreamRejected: IDL.Record({ code: IDL.Text, retryable: IDL.Bool }),
  NotEligible: IDL.Record({ policy_version: IDL.Nat64 }),
  VenueRateLimited: IDL.Record({ retry_after_ms: opt(IDL.Nat64) }),
  RiskLimitExceeded: IDL.Record({ limit: IDL.Nat64 }),
  SessionRevoked: IDL.Null,
  BadRequest: IDL.Record({ code: BadRequestCode, detail: IDL.Text }),
  PolicyUnavailable: IDL.Null,
  SessionExpired: IDL.Null,
  InsufficientFunds: IDL.Record({ requested: IDL.Nat64, available: IDL.Nat64 }),
  Unauthenticated: IDL.Record({ reason: IDL.Text }),
})
const AgentGeneration = IDL.Record({
  account_id: Blob,
  generation: IDL.Nat64,
  approved_at: opt(IDL.Nat64),
  state: IDL.Variant({
    Failed: IDL.Null,
    Active: IDL.Null,
    Expiring: IDL.Null,
    Approving: IDL.Null,
    Requested: IDL.Null,
    Revoked: IDL.Null,
  }),
  agent_address: Blob,
  expires_at: opt(IDL.Nat64),
})
const FundRequestAccepted = IDL.Record({
  request_id: Blob,
  fund_action_id: opt(Blob),
  state: IDL.Variant({
    Reserved: IDL.Null,
    Executing: IDL.Null,
    Rejected: IDL.Null,
    Accepted: IDL.Null,
    Unknown: IDL.Null,
    Settled: IDL.Null,
  }),
  accepted_at: IDL.Nat64,
})
const EligibilityClaims = IDL.Record({
  principal: IDL.Principal,
  user_id: Blob,
  account_id: Blob,
  network: IDL.Variant({ Local: IDL.Null, Testnet: IDL.Null, Mainnet: IDL.Null }),
  vault: IDL.Principal,
  terms_version: IDL.Nat64,
  issued_at: IDL.Nat64,
  expires_at: IDL.Nat64,
  nonce: Blob,
})
const EligibilityStatus = IDL.Record({
  terms_version: IDL.Nat64,
  expires_at: opt(IDL.Nat64),
  eligible: IDL.Bool,
})
const EligibilityToken = IDL.Record({ claims: EligibilityClaims, signature: Blob })
const BuilderFeeClaims = IDL.Record({
  principal: IDL.Principal,
  user_id: Blob,
  account_id: Blob,
  vault: IDL.Principal,
  network: IDL.Variant({ Local: IDL.Null, Testnet: IDL.Null, Mainnet: IDL.Null }),
  builder_address: Blob,
  fee_decibps: IDL.Nat16,
  issued_at: IDL.Nat64,
  expires_at: IDL.Nat64,
  nonce: Blob,
})
const BuilderFeeStatus = IDL.Record({
  approved: IDL.Bool,
  builder_address: opt(Blob),
  expires_at: opt(IDL.Nat64),
  approval_records: IDL.Nat64,
  charged_micros: IDL.Nat64,
})
const BuilderFeeConsent = IDL.Record({ claims: BuilderFeeClaims, eoa_signature: Blob })
const result = <T>(type: IDL.Type<T>, bytes: Uint8Array): T =>
  unwrap(decode<{ Ok: T } | { Err: unknown }>(IDL.Variant({ Ok: type, Err: ErrorCode }), bytes))

export const vaultPrivateCodec = {
  session: (session: SessionHandle) => encode(Session, session),
  account: (bytes: Uint8Array): Uint8Array => Uint8Array.from(result(Blob, bytes)),
  eligibilitySigningQuery: (session: SessionHandle, expiresAt: bigint) =>
    new Uint8Array(IDL.encode([Session, IDL.Nat64], [session, expiresAt])),
  eligibilityClaims: (bytes: Uint8Array): Uint8Array =>
    encode(EligibilityClaims, result(EligibilityClaims, bytes)),
  eligibilityRegister: (
    session: SessionHandle,
    claimsCandid: Uint8Array,
    signature: Uint8Array,
  ) => {
    const [claims] = IDL.decode([EligibilityClaims], claimsCandid)
    return new Uint8Array(IDL.encode([Session, EligibilityToken], [session, { claims, signature }]))
  },
  eligibilityStatus: (bytes: Uint8Array) => result(EligibilityStatus, bytes),
  builderFeeSigningQuery: (session: SessionHandle, builder: Uint8Array, expiresAt: bigint) =>
    new Uint8Array(IDL.encode([Session, Blob, IDL.Nat64], [session, builder, expiresAt])),
  builderFeeTarget: (bytes: Uint8Array) => result(IDL.Tuple(BuilderFeeClaims, Blob), bytes),
  builderFeeRegister: (session: SessionHandle, claims: unknown, signature: Uint8Array) =>
    new Uint8Array(
      IDL.encode([Session, BuilderFeeConsent], [session, { claims, eoa_signature: signature }]),
    ),
  builderFeeStatus: (bytes: Uint8Array) => result(BuilderFeeStatus, bytes),
  approveAgent: (session: SessionHandle, generation: bigint, address: Uint8Array | number[]) =>
    new Uint8Array(IDL.encode([Session, IDL.Nat64, Blob], [session, generation, address])),
  allocation: (session: SessionHandle, id: Uint8Array, amount: bigint) =>
    encode(
      IDL.Record({
        session: Session,
        client_request_id: Blob,
        amount: IDL.Nat64,
        target: IDL.Variant({ Reserve: IDL.Null, Trading: IDL.Null }),
        intent_signature: opt(Blob),
      }),
      { session, client_request_id: id, amount, target: { Trading: null }, intent_signature: [] },
    ),
  recovery: (session: SessionHandle, id: Uint8Array, amount: bigint) =>
    new Uint8Array(IDL.encode([Session, Blob, IDL.Nat64], [session, id, amount])),
  withdrawal: (request: unknown) =>
    encode(
      IDL.Record({
        session: Session,
        client_request_id: Blob,
        amount: IDL.Nat64,
        asset: IDL.Variant({ Usdc: IDL.Null, BtcPerp: IDL.Null, EthPerp: IDL.Null }),
        destination: IDL.Variant({ AuthenticatedEoaHlAccount: IDL.Null }),
        network: IDL.Variant({ Mainnet: IDL.Null, Local: IDL.Null, Testnet: IDL.Null }),
        nonce: IDL.Nat64,
        expires_at: IDL.Nat64,
        intent_signature: Blob,
      }),
      request,
    ),
  empty: (bytes: Uint8Array) => result(IDL.Null, bytes),
  agent: (bytes: Uint8Array): AgentGenerationView =>
    result(AgentGeneration, bytes) as AgentGenerationView,
  fund: (
    bytes: Uint8Array,
  ): {
    request_id: Uint8Array
    fund_action_id: [] | [Uint8Array]
    state: FundRequestState
    accepted_at: bigint
  } =>
    result(FundRequestAccepted, bytes) as {
      request_id: Uint8Array
      fund_action_id: [] | [Uint8Array]
      state: FundRequestState
      accepted_at: bigint
    },
}

const SubmitArgs = IDL.Record({
  account_id: Blob,
  limit_price: opt(IDL.Text),
  trigger: opt(Trigger),
  leverage: opt(IDL.Nat32),
  client_request_id: Blob,
  reduce_only: IDL.Bool,
  kind: OrderKind,
  side: Side,
  slippage_tolerance_bps: opt(IDL.Nat32),
  session: Session,
  quantity: IDL.Text,
  market: IDL.Text,
  expires_after: opt(IDL.Nat64),
})
export type SubmitOrderResult = {
  request_id: Uint8Array | number[]
  cloid: Uint8Array | number[]
  accepted_at: bigint
  order_id: Uint8Array | number[]
}
export type CloseAllOutcome = {
  submitted: SubmitOrderResult[]
  failed: { error: unknown; market: string }[]
}
const SubmitResult = IDL.Record({
  request_id: Blob,
  cloid: Blob,
  accepted_at: IDL.Nat64,
  order_id: Blob,
})
const CloseOutcome = IDL.Record({
  submitted: IDL.Vec(SubmitResult),
  failed: IDL.Vec(IDL.Record({ error: ErrorCode, market: IDL.Text })),
})
const wrapCoreArgs = (types: IDL.Type<unknown>[], args: unknown[]) =>
  encode(Blob, new Uint8Array(IDL.encode(types, args)))

export const corePrivateCodec = {
  submit: (session: SessionHandle, args: unknown) =>
    wrapCoreArgs([Session, SubmitArgs], [session, args]),
  closePosition: (
    session: SessionHandle,
    id: Uint8Array,
    market: string,
    ratio: number,
    price: [] | [string],
  ) =>
    wrapCoreArgs(
      [Session, Blob, IDL.Text, IDL.Nat32, opt(IDL.Text)],
      [session, id, market, ratio, price],
    ),
  closeAll: (session: SessionHandle, id: Uint8Array) =>
    wrapCoreArgs([Session, Blob], [session, id]),
  session: (session: SessionHandle) => wrapCoreArgs([Session], [session]),
  submitResult: (bytes: Uint8Array): SubmitOrderResult =>
    result(SubmitResult, bytes) as SubmitOrderResult,
  closeOutcome: (bytes: Uint8Array): CloseAllOutcome =>
    result(CloseOutcome, bytes) as CloseAllOutcome,
  agent: (bytes: Uint8Array): AgentGenerationView =>
    result(AgentGeneration, bytes) as AgentGenerationView,
  count: (bytes: Uint8Array): bigint => BigInt(result(IDL.Nat64, bytes)),
}
