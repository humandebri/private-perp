// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/trading_core.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
export const idlFactory = ({ IDL }) => {
  const RecoveryFenceToken = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    user_id: IDL.Vec(IDL.Nat8),
    epoch: IDL.Nat64,
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
  const Result = IDL.Variant({ Ok: IDL.Null, Err: ErrorCode })
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
  const MarketThreshold = IDL.Record({
    max_spread_bps: IDL.Nat32,
    min_day_notional_usdc: IDL.Nat64,
    expected_index: IDL.Nat32,
    market: IDL.Text,
    min_each_side_depth_usdc: IDL.Nat64,
  })
  const SessionHandle = IDL.Record({
    session_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
    vault_principal: IDL.Principal,
    revocation_generation: IDL.Nat64,
  })
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
  const Result_2 = IDL.Variant({ Ok: AgentStatus, Err: ErrorCode })
  const CyclesStatus = IDL.Record({
    warning: IDL.Bool,
    observed_daily_burn: IDL.Nat,
    balance: IDL.Nat,
    refill_target: IDL.Opt(IDL.Nat),
    exit_reserve: IDL.Opt(IDL.Nat),
    configured_daily_floor: IDL.Opt(IDL.Nat),
    estimated_days: IDL.Opt(IDL.Nat64),
    new_risk_stopped: IDL.Bool,
    observed_at: IDL.Nat64,
  })
  const Result_3 = IDL.Variant({ Ok: CyclesStatus, Err: ErrorCode })
  const EnvironmentView = IDL.Record({
    ecdsa_key_id: IDL.Text,
    info_url: IDL.Text,
    network: Network,
    exchange_url: IDL.Text,
  })
  const Result_4 = IDL.Variant({ Ok: EnvironmentView, Err: ErrorCode })
  const Result_5 = IDL.Variant({ Ok: IDL.Vec(IDL.Nat8), Err: ErrorCode })
  const MarketStatus = IDL.Record({
    eligible_for_new_risk: IDL.Bool,
    market: IDL.Text,
    reason_code: IDL.Opt(IDL.Text),
    observed_at: IDL.Opt(IDL.Nat64),
  })
  const Result_6 = IDL.Variant({ Ok: MarketStatus, Err: ErrorCode })
  const Result_7 = IDL.Variant({
    Ok: IDL.Opt(IDL.Principal),
    Err: ErrorCode,
  })
  const Result_8 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Nat64, IDL.Bool),
    Err: ErrorCode,
  })
  const Result_13 = IDL.Variant({ Ok: IDL.Tuple(IDL.Bool, IDL.Bool), Err: ErrorCode })
  const PrepareRecovery = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    user_id: IDL.Vec(IDL.Nat8),
    master_address: IDL.Vec(IDL.Nat8),
  })
  const Result_9 = IDL.Variant({
    Ok: RecoveryFenceToken,
    Err: ErrorCode,
  })
  const Result_10 = IDL.Variant({ Ok: IDL.Bool, Err: ErrorCode })
  const PreflightResolution = IDL.Variant({
    Applied: IDL.Null,
    Rejected: IDL.Null,
  })
  const SweepOutcome = IDL.Record({
    reconciled: IDL.Nat32,
    dispatched: IDL.Nat32,
    cancels: IDL.Nat32,
  })
  const Result_11 = IDL.Variant({ Ok: SweepOutcome, Err: ErrorCode })
  const Result_12 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Bool),
    Err: ErrorCode,
  })
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
    abort_recovery: IDL.Func([RecoveryFenceToken], [Result], []),
    begin_recovery_migration: IDL.Func([], [Result], []),
    cancel_order: IDL.Func([HpkeRequest], [Result_1], []),
    commit_recovery: IDL.Func([RecoveryFenceToken], [Result], []),
    configure_cycles: IDL.Func([IDL.Nat, IDL.Nat], [Result], []),
    configure_market_threshold: IDL.Func([MarketThreshold], [Result], []),
    finish_recovery: IDL.Func([RecoveryFenceToken], [Result], []),
    finish_recovery_migration: IDL.Func([], [Result], []),
    get_account_snapshot: IDL.Func([HpkeRequest], [Result_1], []),
    get_agent_status: IDL.Func([SessionHandle], [Result_2], []),
    get_cycles_status: IDL.Func([], [Result_3], []),
    get_environment: IDL.Func([], [Result_4], ['query']),
    get_hpke_public_key: IDL.Func([], [Result_5], ['query']),
    get_journal_guard: IDL.Func([], [Result_7], ['query']),
    get_journal_send_status: IDL.Func([], [Result_13], ['query']),
    get_market_status: IDL.Func([IDL.Text], [Result_6], ['query']),
    get_order_by_request: IDL.Func([HpkeRequest], [Result_1], []),
    get_policy_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    get_send_journal: IDL.Func([], [Result_7], ['query']),
    get_vault_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    journal_restore_status: IDL.Func([], [Result_8], ['query']),
    recovery_replay_pending: IDL.Func([], [Result_10], ['query']),
    recovery_stage_status: IDL.Func([], [Result_12], ['query']),
    list_fills: IDL.Func([HpkeRequest], [Result_1], []),
    list_orders: IDL.Func([HpkeRequest], [Result_1], []),
    mark_recovery_unknown: IDL.Func([RecoveryFenceToken], [Result], []),
    migrate_recovery: IDL.Func([PrepareRecovery], [Result_9], []),
    prepare_recovery: IDL.Func([PrepareRecovery], [Result_9], []),
    private_call: IDL.Func([HpkeRequest], [Result_1], []),
    recovery_migration_locked: IDL.Func([], [Result_10], ['query']),
    refresh_market: IDL.Func([], [Result], []),
    resolve_unknown_order_preflight: IDL.Func(
      [IDL.Vec(IDL.Nat8), PreflightResolution],
      [Result],
      [],
    ),
    resume_journal: IDL.Func([], [Result], []),
    rotate_hpke_key: IDL.Func([], [Result_5], []),
    set_ecdsa_key_id: IDL.Func([IDL.Text], [Result], []),
    set_journal_guard: IDL.Func([IDL.Principal], [Result], []),
    set_market_context: IDL.Func([IDL.Text, IDL.Text], [Result], []),
    set_meta_cache: IDL.Func([IDL.Text, IDL.Text, IDL.Text], [Result], []),
    set_policy_principal: IDL.Func([IDL.Principal], [Result], []),
    set_send_journal: IDL.Func([IDL.Principal], [Result], []),
    set_vault_principal: IDL.Func([IDL.Principal], [Result], []),
    set_venue_endpoints: IDL.Func([IDL.Text, IDL.Text], [Result], []),
    sweep: IDL.Func([], [Result_11], []),
    transform_info: IDL.Func([TransformArgs], [HttpRequestResult], ['query']),
    transform_market_info: IDL.Func([TransformArgs], [HttpRequestResult], ['query']),
    transform_open_orders: IDL.Func([TransformArgs], [HttpRequestResult], ['query']),
    version: IDL.Func([], [IDL.Text], ['query']),
    whoami: IDL.Func([SessionHandle], [Result_5], []),
  })
}
export const init = ({ IDL }) => {
  return []
}
