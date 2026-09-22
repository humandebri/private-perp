// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/trading_core.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
export const idlFactory = ({ IDL }) => {
  const SessionHandle = IDL.Record({
    session_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
    vault_principal: IDL.Principal,
    revocation_generation: IDL.Nat64,
  })
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
    DuplicateIgnored: IDL.Record({ request_id: IDL.Vec(IDL.Nat8) }),
    SigningQueueFull: IDL.Null,
    NotAllowed: IDL.Record({ code: NotAllowedCode }),
    UpstreamUnavailable: IDL.Record({ venue: IDL.Text }),
    UnknownPending: IDL.Record({ action_id: IDL.Vec(IDL.Nat8) }),
    ReservationConflict: IDL.Null,
    StaleAccountState: IDL.Record({
      max_age_ms: IDL.Nat64,
      observed_at: IDL.Nat64,
    }),
    IdempotencyConflict: IDL.Record({ request_id: IDL.Vec(IDL.Nat8) }),
    UpstreamRejected: IDL.Record({
      code: IDL.Text,
      retryable: IDL.Bool,
    }),
    NotEligible: IDL.Record({ policy_version: IDL.Nat64 }),
    VenueRateLimited: IDL.Record({ retry_after_ms: IDL.Opt(IDL.Nat64) }),
    RiskLimitExceeded: IDL.Record({ limit: IDL.Nat64 }),
    SessionRevoked: IDL.Null,
    BadRequest: IDL.Record({ code: BadRequestCode, detail: IDL.Text }),
    PolicyUnavailable: IDL.Null,
    SessionExpired: IDL.Null,
    InsufficientFunds: IDL.Record({
      requested: IDL.Nat64,
      available: IDL.Nat64,
    }),
    Unauthenticated: IDL.Record({ reason: IDL.Text }),
  })
  const Result = IDL.Variant({ Ok: IDL.Nat64, Err: ErrorCode })
  const Network = IDL.Variant({
    Mainnet: IDL.Null,
    Local: IDL.Null,
    Testnet: IDL.Null,
  })
  const HpkeRequest = IDL.Record({
    aad: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    method: IDL.Text,
    ciphertext: IDL.Vec(IDL.Nat8),
    key_id: IDL.Vec(IDL.Nat8),
    network: Network,
    client_public_key: IDL.Vec(IDL.Nat8),
    canister: IDL.Principal,
    expires_at: IDL.Nat64,
  })
  const HpkeResponse = IDL.Record({
    request_id: IDL.Vec(IDL.Nat8),
    ciphertext: IDL.Vec(IDL.Nat8),
    key_id: IDL.Vec(IDL.Nat8),
    observed_at: IDL.Nat64,
  })
  const Result_1 = IDL.Variant({ Ok: HpkeResponse, Err: ErrorCode })
  const SubmitOrderResult = IDL.Record({
    request_id: IDL.Vec(IDL.Nat8),
    cloid: IDL.Vec(IDL.Nat8),
    accepted_at: IDL.Nat64,
    order_id: IDL.Vec(IDL.Nat8),
  })
  const CloseFailure = IDL.Record({ error: ErrorCode, market: IDL.Text })
  const CloseAllOutcome = IDL.Record({
    submitted: IDL.Vec(SubmitOrderResult),
    failed: IDL.Vec(CloseFailure),
  })
  const Result_2 = IDL.Variant({ Ok: CloseAllOutcome, Err: ErrorCode })
  const Result_3 = IDL.Variant({ Ok: SubmitOrderResult, Err: ErrorCode })
  const AgentState = IDL.Variant({
    Failed: IDL.Null,
    Active: IDL.Null,
    Expiring: IDL.Null,
    Approving: IDL.Null,
    Requested: IDL.Null,
    Revoked: IDL.Null,
  })
  const AgentGeneration = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    generation: IDL.Nat64,
    approved_at: IDL.Opt(IDL.Nat64),
    state: AgentState,
    agent_address: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Opt(IDL.Nat64),
  })
  const AgentStatus = IDL.Record({
    revocation_pending: IDL.Bool,
    next: IDL.Opt(AgentGeneration),
    current: IDL.Opt(AgentGeneration),
    observed_at: IDL.Nat64,
  })
  const Result_4 = IDL.Variant({ Ok: AgentStatus, Err: ErrorCode })
  const EnvironmentView = IDL.Record({
    ecdsa_key_id: IDL.Text,
    info_url: IDL.Text,
    network: Network,
    exchange_url: IDL.Text,
  })
  const Result_5 = IDL.Variant({ Ok: EnvironmentView, Err: ErrorCode })
  const Result_6 = IDL.Variant({ Ok: IDL.Vec(IDL.Nat8), Err: ErrorCode })
  const Result_7 = IDL.Variant({ Ok: AgentGeneration, Err: ErrorCode })
  const PreflightResolution = IDL.Variant({
    Applied: IDL.Null,
    Rejected: IDL.Null,
  })
  const Result_8 = IDL.Variant({ Ok: IDL.Null, Err: ErrorCode })
  const TriggerKind = IDL.Variant({
    TakeProfit: IDL.Null,
    StopLoss: IDL.Null,
  })
  const Trigger = IDL.Record({
    kind: TriggerKind,
    is_market: IDL.Bool,
    trigger_price: IDL.Text,
  })
  const OrderKind = IDL.Variant({
    LimitGtc: IDL.Null,
    MarketIoc: IDL.Null,
  })
  const Side = IDL.Variant({ Buy: IDL.Null, Sell: IDL.Null })
  const SubmitOrderArgs = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    limit_price: IDL.Opt(IDL.Text),
    trigger: IDL.Opt(Trigger),
    leverage: IDL.Opt(IDL.Nat32),
    client_request_id: IDL.Vec(IDL.Nat8),
    reduce_only: IDL.Bool,
    kind: OrderKind,
    side: Side,
    slippage_tolerance_bps: IDL.Opt(IDL.Nat32),
    session: SessionHandle,
    quantity: IDL.Text,
    market: IDL.Text,
    expires_after: IDL.Opt(IDL.Nat64),
  })
  const SweepOutcome = IDL.Record({
    reconciled: IDL.Nat32,
    dispatched: IDL.Nat32,
    cancels: IDL.Nat32,
  })
  const Result_9 = IDL.Variant({ Ok: SweepOutcome, Err: ErrorCode })
  const HttpHeader = IDL.Record({ value: IDL.Text, name: IDL.Text })
  const HttpRequestResult = IDL.Record({
    status: IDL.Nat,
    body: IDL.Vec(IDL.Nat8),
    headers: IDL.Vec(HttpHeader),
  })
  const TransformArgs = IDL.Record({
    context: IDL.Vec(IDL.Nat8),
    response: HttpRequestResult,
  })
  return IDL.Service({
    cancel_all: IDL.Func([SessionHandle], [Result], []),
    cancel_order: IDL.Func([HpkeRequest], [Result_1], []),
    close_all: IDL.Func([SessionHandle, IDL.Vec(IDL.Nat8)], [Result_2], []),
    close_position: IDL.Func(
      [SessionHandle, IDL.Vec(IDL.Nat8), IDL.Text, IDL.Nat32, IDL.Opt(IDL.Text)],
      [Result_3],
      [],
    ),
    get_account_snapshot: IDL.Func([HpkeRequest], [Result_1], []),
    get_agent_status: IDL.Func([SessionHandle], [Result_4], []),
    get_environment: IDL.Func([], [Result_5], ['query']),
    get_hpke_public_key: IDL.Func([], [Result_6], ['query']),
    get_order_by_request: IDL.Func([HpkeRequest], [Result_1], []),
    get_policy_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    get_vault_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    list_fills: IDL.Func([HpkeRequest], [Result_1], []),
    list_orders: IDL.Func([HpkeRequest], [Result_1], []),
    request_agent_generation: IDL.Func([SessionHandle], [Result_7], []),
    resolve_unknown_order_preflight: IDL.Func(
      [IDL.Vec(IDL.Nat8), PreflightResolution],
      [Result_8],
      [],
    ),
    rotate_hpke_key: IDL.Func([], [Result_6], []),
    set_ecdsa_key_id: IDL.Func([IDL.Text], [Result_8], []),
    set_market_context: IDL.Func([IDL.Text, IDL.Text], [Result_8], []),
    set_meta_cache: IDL.Func([IDL.Text, IDL.Text, IDL.Text], [Result_8], []),
    set_policy_principal: IDL.Func([IDL.Principal], [Result_8], []),
    set_vault_principal: IDL.Func([IDL.Principal], [Result_8], []),
    set_venue_endpoints: IDL.Func([IDL.Text, IDL.Text], [Result_8], []),
    submit_order: IDL.Func([SessionHandle, SubmitOrderArgs], [Result_3], []),
    sweep: IDL.Func([], [Result_9], []),
    transform_info: IDL.Func([TransformArgs], [HttpRequestResult], ['query']),
    version: IDL.Func([], [IDL.Text], ['query']),
    whoami: IDL.Func([SessionHandle], [Result_6], []),
  })
}
export const init = ({ IDL }) => {
  return []
}
