// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/private_perp.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
export const idlFactory = ({ IDL }) => {
  const core_RecoveryFenceToken = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    user_id: IDL.Vec(IDL.Nat8),
    epoch: IDL.Nat64,
  })
  const core_NotAllowedCode = IDL.Variant({
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
  const core_BadRequestCode = IDL.Variant({
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
  const core_ErrorCode = IDL.Variant({
    Internal: IDL.Record({ code: IDL.Text }),
    JournalWriterBusy: IDL.Null,
    DuplicateIgnored: IDL.Record({ request_id: IDL.Vec(IDL.Nat8) }),
    SigningQueueFull: IDL.Null,
    NotAllowed: IDL.Record({ code: core_NotAllowedCode }),
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
    BadRequest: IDL.Record({
      code: core_BadRequestCode,
      detail: IDL.Text,
    }),
    PolicyUnavailable: IDL.Null,
    SessionExpired: IDL.Null,
    InsufficientFunds: IDL.Record({
      requested: IDL.Nat64,
      available: IDL.Nat64,
    }),
    Unauthenticated: IDL.Record({ reason: IDL.Text }),
  })
  const core_Result = IDL.Variant({ Ok: IDL.Null, Err: core_ErrorCode })
  const journal_SendIntent = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    kind: IDL.Text,
    nonce: IDL.Nat64,
    digest: IDL.Vec(IDL.Nat8),
  })
  const journal_JournalHead = IDL.Record({
    hash: IDL.Vec(IDL.Nat8),
    sequence: IDL.Nat64,
  })
  const journal_NotAllowedCode = IDL.Variant({
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
  const journal_BadRequestCode = IDL.Variant({
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
  const journal_ErrorCode = IDL.Variant({
    Internal: IDL.Record({ code: IDL.Text }),
    JournalWriterBusy: IDL.Null,
    DuplicateIgnored: IDL.Record({ request_id: IDL.Vec(IDL.Nat8) }),
    SigningQueueFull: IDL.Null,
    NotAllowed: IDL.Record({ code: journal_NotAllowedCode }),
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
    BadRequest: IDL.Record({
      code: journal_BadRequestCode,
      detail: IDL.Text,
    }),
    PolicyUnavailable: IDL.Null,
    SessionExpired: IDL.Null,
    InsufficientFunds: IDL.Record({
      requested: IDL.Nat64,
      available: IDL.Nat64,
    }),
    Unauthenticated: IDL.Record({ reason: IDL.Text }),
  })
  const journal_Result = IDL.Variant({
    Ok: journal_JournalHead,
    Err: journal_ErrorCode,
  })
  const journal_RecoveryPayload = IDL.Variant({
    CustodyAccount: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      kind: IDL.Text,
      derivation_path: IDL.Text,
      network: IDL.Text,
      user_id: IDL.Opt(IDL.Vec(IDL.Nat8)),
      address: IDL.Vec(IDL.Nat8),
    }),
    OrderRisk: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      state: IDL.Text,
      risk_micros: IDL.Nat64,
      order_id: IDL.Vec(IDL.Nat8),
    }),
    WithdrawalAccepted: IDL.Record({
      request_id: IDL.Vec(IDL.Nat8),
      action_id: IDL.Vec(IDL.Nat8),
      destination: IDL.Vec(IDL.Nat8),
      body_hash: IDL.Vec(IDL.Nat8),
      intent_expires_at_ms: IDL.Nat64,
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      nonce: IDL.Nat64,
      accepted_at_ms: IDL.Nat64,
      reserve_account_id: IDL.Vec(IDL.Nat8),
      intent_nonce: IDL.Nat64,
    }),
    Fill: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      fill_id: IDL.Vec(IDL.Nat8),
      size_micros: IDL.Nat64,
      order_id: IDL.Vec(IDL.Nat8),
      price_micros: IDL.Nat64,
    }),
    RecoveryAccepted: IDL.Record({
      request_id: IDL.Vec(IDL.Nat8),
      action_id: IDL.Vec(IDL.Nat8),
      destination: IDL.Vec(IDL.Nat8),
      body_hash: IDL.Vec(IDL.Nat8),
      trading_account_id: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      nonce: IDL.Nat64,
      accepted_at_ms: IDL.Nat64,
      reserve_account_id: IDL.Vec(IDL.Nat8),
    }),
    OrderActionResult: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      hl_oid: IDL.Opt(IDL.Nat64),
      client_request_id: IDL.Vec(IDL.Nat8),
      kind: IDL.Text,
      observed_at_ms: IDL.Nat64,
      filled: IDL.Bool,
      order_id: IDL.Vec(IDL.Nat8),
      accepted: IDL.Bool,
    }),
    IdentityRegistration: IDL.Record({
      owner: IDL.Principal,
      network: IDL.Text,
      user_id: IDL.Vec(IDL.Nat8),
      eoa_address: IDL.Vec(IDL.Nat8),
    }),
    Reservation: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      request_id: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      state: IDL.Text,
    }),
    OrderStatusObserved: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      hl_oid: IDL.Nat64,
      observed_at_ms: IDL.Nat64,
      state: IDL.Text,
      evidence_digest: IDL.Vec(IDL.Nat8),
      order_id: IDL.Vec(IDL.Nat8),
    }),
    AllocationAccepted: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      request_id: IDL.Vec(IDL.Nat8),
      action_id: IDL.Vec(IDL.Nat8),
      destination: IDL.Vec(IDL.Nat8),
      body_hash: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      nonce: IDL.Nat64,
      accepted_at_ms: IDL.Nat64,
    }),
    RecoverySettlement: IDL.Record({
      request_id: IDL.Vec(IDL.Nat8),
      action_id: IDL.Vec(IDL.Nat8),
      observed_at_ms: IDL.Nat64,
      trading_account_id: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      evidence_digest: IDL.Vec(IDL.Nat8),
      nonce: IDL.Nat64,
      accepted: IDL.Bool,
    }),
    OrderAccepted: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      request_id: IDL.Vec(IDL.Nat8),
      body_hash: IDL.Vec(IDL.Nat8),
      reduce_only: IDL.Bool,
      cloid: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      risk_micros: IDL.Nat64,
      accepted_at_ms: IDL.Nat64,
      order_id: IDL.Vec(IDL.Nat8),
    }),
    IdentityAccount: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      owner: IDL.Principal,
      user_id: IDL.Vec(IDL.Nat8),
      address: IDL.Vec(IDL.Nat8),
    }),
    FundTransferResult: IDL.Record({
      request_id: IDL.Vec(IDL.Nat8),
      action_id: IDL.Vec(IDL.Nat8),
      destination: IDL.Vec(IDL.Nat8),
      kind: IDL.Text,
      observed_at_ms: IDL.Nat64,
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      evidence_digest: IDL.Vec(IDL.Nat8),
      nonce: IDL.Nat64,
      source_account_id: IDL.Vec(IDL.Nat8),
      accepted: IDL.Bool,
    }),
    ExternalOutcome: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      request_id: IDL.Vec(IDL.Nat8),
      kind: IDL.Text,
      state: IDL.Text,
      evidence_digest: IDL.Vec(IDL.Nat8),
    }),
    TradingBalanceObserved: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      previous_equity: IDL.Nat64,
      observed_at_ms: IDL.Nat64,
      user_id: IDL.Vec(IDL.Nat8),
      equity: IDL.Nat64,
    }),
    DepositClaim: IDL.Record({
      claimed_at_ms: IDL.Nat64,
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      event_id: IDL.Vec(IDL.Nat8),
    }),
    Baseline: IDL.Record({ state_digest: IDL.Vec(IDL.Nat8) }),
    LedgerPosting: IDL.Record({
      account_id: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Int64,
      category: IDL.Text,
      posting_id: IDL.Vec(IDL.Nat8),
    }),
    FillObserved: IDL.Record({
      fee: IDL.Int64,
      tid: IDL.Nat64,
      account_id: IDL.Vec(IDL.Nat8),
      hl_oid: IDL.Nat64,
      filled_at_ms: IDL.Nat64,
      user_id: IDL.Vec(IDL.Nat8),
      quantity: IDL.Text,
      market: IDL.Text,
      order_id: IDL.Vec(IDL.Nat8),
      price: IDL.Text,
    }),
    RecoveryPostResult: IDL.Record({
      request_id: IDL.Vec(IDL.Nat8),
      action_id: IDL.Vec(IDL.Nat8),
      observed_at_ms: IDL.Nat64,
      trading_account_id: IDL.Vec(IDL.Nat8),
      user_id: IDL.Vec(IDL.Nat8),
      amount_micros: IDL.Nat64,
      evidence_digest: IDL.Vec(IDL.Nat8),
      nonce: IDL.Nat64,
      accepted: IDL.Bool,
    }),
    DepositCredit: IDL.Record({
      observed_at_ms: IDL.Nat64,
      network: IDL.Text,
      sender: IDL.Opt(IDL.Vec(IDL.Nat8)),
      amount_micros: IDL.Nat64,
      address: IDL.Vec(IDL.Nat8),
      tx_hash: IDL.Vec(IDL.Nat8),
    }),
  })
  const journal_RecoveryEvent = IDL.Record({
    logical_id: IDL.Vec(IDL.Nat8),
    version: IDL.Nat16,
    payload: journal_RecoveryPayload,
  })
  const vault_SessionHandle = IDL.Record({
    session_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
    vault_principal: IDL.Principal,
    revocation_generation: IDL.Nat64,
  })
  const vault_BuilderFeeMockStatus = IDL.Record({
    approval_records: IDL.Nat64,
    builder_address: IDL.Opt(IDL.Vec(IDL.Nat8)),
    approved: IDL.Bool,
    charged_micros: IDL.Nat64,
    expires_at: IDL.Opt(IDL.Nat64),
  })
  const vault_NotAllowedCode = IDL.Variant({
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
  const vault_BadRequestCode = IDL.Variant({
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
  const vault_ErrorCode = IDL.Variant({
    Internal: IDL.Record({ code: IDL.Text }),
    JournalWriterBusy: IDL.Null,
    DuplicateIgnored: IDL.Record({ request_id: IDL.Vec(IDL.Nat8) }),
    SigningQueueFull: IDL.Null,
    NotAllowed: IDL.Record({ code: vault_NotAllowedCode }),
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
    BadRequest: IDL.Record({
      code: vault_BadRequestCode,
      detail: IDL.Text,
    }),
    PolicyUnavailable: IDL.Null,
    SessionExpired: IDL.Null,
    InsufficientFunds: IDL.Record({
      requested: IDL.Nat64,
      available: IDL.Nat64,
    }),
    Unauthenticated: IDL.Record({ reason: IDL.Text }),
  })
  const vault_Result = IDL.Variant({
    Ok: vault_BuilderFeeMockStatus,
    Err: vault_ErrorCode,
  })
  const core_Network = IDL.Variant({
    Mainnet: IDL.Null,
    Local: IDL.Null,
    Testnet: IDL.Null,
  })
  const core_HpkeRequest = IDL.Record({
    aad: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    method: IDL.Text,
    ciphertext: IDL.Vec(IDL.Nat8),
    key_id: IDL.Vec(IDL.Nat8),
    network: core_Network,
    client_public_key: IDL.Vec(IDL.Nat8),
    canister: IDL.Principal,
    expires_at: IDL.Nat64,
  })
  const core_HpkeResponse = IDL.Record({
    request_id: IDL.Vec(IDL.Nat8),
    ciphertext: IDL.Vec(IDL.Nat8),
    key_id: IDL.Vec(IDL.Nat8),
    observed_at: IDL.Nat64,
  })
  const core_Result_1 = IDL.Variant({
    Ok: core_HpkeResponse,
    Err: core_ErrorCode,
  })
  const vault_Result_1 = IDL.Variant({
    Ok: IDL.Null,
    Err: vault_ErrorCode,
  })
  const policy_NotAllowedCode = IDL.Variant({
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
  const policy_BadRequestCode = IDL.Variant({
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
  const policy_ErrorCode = IDL.Variant({
    Internal: IDL.Record({ code: IDL.Text }),
    JournalWriterBusy: IDL.Null,
    DuplicateIgnored: IDL.Record({ request_id: IDL.Vec(IDL.Nat8) }),
    SigningQueueFull: IDL.Null,
    NotAllowed: IDL.Record({ code: policy_NotAllowedCode }),
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
    BadRequest: IDL.Record({
      code: policy_BadRequestCode,
      detail: IDL.Text,
    }),
    PolicyUnavailable: IDL.Null,
    SessionExpired: IDL.Null,
    InsufficientFunds: IDL.Record({
      requested: IDL.Nat64,
      available: IDL.Nat64,
    }),
    Unauthenticated: IDL.Record({ reason: IDL.Text }),
  })
  const policy_Result = IDL.Variant({
    Ok: IDL.Null,
    Err: policy_ErrorCode,
  })
  const policy_BudgetClass = IDL.Variant({
    Exit: IDL.Null,
    Reconcile: IDL.Null,
    NewRisk: IDL.Null,
  })
  const policy_RestBudgetRequest = IDL.Record({
    request_id: IDL.Vec(IDL.Nat8),
    weight: IDL.Nat32,
    class: policy_BudgetClass,
    expires_at: IDL.Nat64,
  })
  const core_MarketThreshold = IDL.Record({
    max_spread_bps: IDL.Nat32,
    min_day_notional_usdc: IDL.Nat64,
    expected_index: IDL.Nat32,
    market: IDL.Text,
    min_each_side_depth_usdc: IDL.Nat64,
  })
  const core_CyclesStatus = IDL.Record({
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
  const core_Result_3 = IDL.Variant({
    Ok: core_CyclesStatus,
    Err: core_ErrorCode,
  })
  const core_EnvironmentView = IDL.Record({
    ecdsa_key_id: IDL.Text,
    info_url: IDL.Text,
    network: core_Network,
    exchange_url: IDL.Text,
  })
  const core_Result_4 = IDL.Variant({
    Ok: core_EnvironmentView,
    Err: core_ErrorCode,
  })
  const core_Result_5 = IDL.Variant({
    Ok: IDL.Vec(IDL.Nat8),
    Err: core_ErrorCode,
  })
  const core_Result_7 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Bool, IDL.Bool),
    Err: core_ErrorCode,
  })
  const core_Result_6 = IDL.Variant({
    Ok: IDL.Opt(IDL.Principal),
    Err: core_ErrorCode,
  })
  const core_Result_9 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Nat64, IDL.Bool),
    Err: core_ErrorCode,
  })
  const core_Result_11 = IDL.Variant({
    Ok: IDL.Bool,
    Err: core_ErrorCode,
  })
  const core_Result_12 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Bool),
    Err: core_ErrorCode,
  })
  const vault_EligibilityStatus = IDL.Record({
    terms_version: IDL.Nat64,
    eligible: IDL.Bool,
    expires_at: IDL.Opt(IDL.Nat64),
  })
  const vault_Result_2 = IDL.Variant({
    Ok: vault_EligibilityStatus,
    Err: vault_ErrorCode,
  })
  const vault_AgentState = IDL.Variant({
    Failed: IDL.Null,
    Active: IDL.Null,
    Expiring: IDL.Null,
    Approving: IDL.Null,
    Requested: IDL.Null,
    Revoked: IDL.Null,
  })
  const vault_AgentGeneration = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    generation: IDL.Nat64,
    approved_at: IDL.Opt(IDL.Nat64),
    state: vault_AgentState,
    agent_address: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Opt(IDL.Nat64),
  })
  const vault_Result_3 = IDL.Variant({
    Ok: IDL.Opt(vault_AgentGeneration),
    Err: vault_ErrorCode,
  })
  const core_SessionHandle = IDL.Record({
    session_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
    vault_principal: IDL.Principal,
    revocation_generation: IDL.Nat64,
  })
  const core_AgentState = IDL.Variant({
    Failed: IDL.Null,
    Active: IDL.Null,
    Expiring: IDL.Null,
    Approving: IDL.Null,
    Requested: IDL.Null,
    Revoked: IDL.Null,
  })
  const core_AgentGeneration = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    generation: IDL.Nat64,
    approved_at: IDL.Opt(IDL.Nat64),
    state: core_AgentState,
    agent_address: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Opt(IDL.Nat64),
  })
  const core_AgentStatus = IDL.Record({
    revocation_pending: IDL.Bool,
    next: IDL.Opt(core_AgentGeneration),
    current: IDL.Opt(core_AgentGeneration),
    observed_at: IDL.Nat64,
  })
  const core_Result_2 = IDL.Variant({
    Ok: core_AgentStatus,
    Err: core_ErrorCode,
  })
  const vault_Result_4 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Nat64),
    Err: vault_ErrorCode,
  })
  const vault_Result_6 = IDL.Variant({
    Ok: IDL.Opt(IDL.Tuple(IDL.Nat64, IDL.Vec(IDL.Nat8))),
    Err: vault_ErrorCode,
  })
  const vault_FundActionKind = IDL.Variant({
    AgentRevocation: IDL.Null,
    Recovery: IDL.Null,
    Withdrawal: IDL.Null,
    AgentApproval: IDL.Null,
    Allocation: IDL.Null,
  })
  const vault_ActionState = IDL.Variant({
    Queued: IDL.Null,
    Signing: IDL.Null,
    Reconciled: IDL.Null,
    Dispatching: IDL.Null,
    Unknown: IDL.Null,
    Signed: IDL.Null,
    Aborted: IDL.Null,
  })
  const vault_UnresolvedAction = IDL.Record({
    action_id: IDL.Vec(IDL.Nat8),
    kind: vault_FundActionKind,
    since: IDL.Nat64,
    state: vault_ActionState,
  })
  const vault_RecoveryFenceStatus = IDL.Variant({
    Reconciling: IDL.Null,
    Preparing: IDL.Null,
  })
  const vault_FundStatus = IDL.Record({
    trading_equity: IDL.Nat64,
    in_transit: IDL.Nat64,
    unknowns: IDL.Vec(vault_UnresolvedAction),
    recovery_fence: IDL.Opt(vault_RecoveryFenceStatus),
    trading_unrealized_pnl: IDL.Int64,
    withdrawable: IDL.Nat64,
    reserve_unallocated: IDL.Nat64,
    reserved_for_withdrawal: IDL.Nat64,
    revision: IDL.Nat64,
    observed_at: IDL.Nat64,
  })
  const vault_Result_8 = IDL.Variant({
    Ok: vault_FundStatus,
    Err: vault_ErrorCode,
  })
  const vault_AssetId = IDL.Variant({
    Usdc: IDL.Null,
    BtcPerp: IDL.Null,
    EthPerp: IDL.Null,
  })
  const vault_Network = IDL.Variant({
    Mainnet: IDL.Null,
    Local: IDL.Null,
    Testnet: IDL.Null,
  })
  const vault_AccountKind = IDL.Variant({
    Reserve: IDL.Null,
    Trading: IDL.Null,
  })
  const vault_FundingInstructions = IDL.Record({
    source_hl_account_address: IDL.Vec(IDL.Nat8),
    asset: vault_AssetId,
    network: vault_Network,
    minimum_amount: IDL.Opt(IDL.Nat64),
    hl_account_address: IDL.Vec(IDL.Nat8),
    memo_required: IDL.Bool,
    account_kind: vault_AccountKind,
  })
  const vault_Result_9 = IDL.Variant({
    Ok: vault_FundingInstructions,
    Err: vault_ErrorCode,
  })
  const core_MarketStatus = IDL.Record({
    eligible_for_new_risk: IDL.Bool,
    market: IDL.Text,
    reason_code: IDL.Opt(IDL.Text),
    observed_at: IDL.Opt(IDL.Nat64),
  })
  const core_Result_8 = IDL.Variant({
    Ok: core_MarketStatus,
    Err: core_ErrorCode,
  })
  const policy_Policy = IDL.Record({
    markets: IDL.Vec(IDL.Text),
    version: IDL.Nat64,
  })
  const policy_Result_1 = IDL.Variant({
    Ok: policy_Policy,
    Err: policy_ErrorCode,
  })
  const vault_Result_13 = IDL.Variant({
    Ok: IDL.Bool,
    Err: vault_ErrorCode,
  })
  const policy_RestBudgetConfig = IDL.Record({
    exit_reserve: IDL.Nat32,
    capacity: IDL.Nat32,
  })
  const policy_RestBudgetStatus = IDL.Record({
    new_risk_used: IDL.Nat32,
    recovery_paused: IDL.Bool,
    used: IDL.Nat32,
    config: IDL.Opt(policy_RestBudgetConfig),
  })
  const policy_Result_2 = IDL.Variant({
    Ok: policy_RestBudgetStatus,
    Err: policy_ErrorCode,
  })
  const policy_StopStatus = IDL.Record({
    stopped: IDL.Bool,
    since: IDL.Opt(IDL.Nat64),
    reason: IDL.Opt(IDL.Text),
  })
  const vault_Result_14 = IDL.Variant({
    Ok: IDL.Opt(IDL.Vec(IDL.Nat8)),
    Err: vault_ErrorCode,
  })
  const vault_Result_10 = IDL.Variant({
    Ok: IDL.Vec(IDL.Nat8),
    Err: vault_ErrorCode,
  })
  const journal_JournalRecord = IDL.Record({
    hash: IDL.Vec(IDL.Nat8),
    previous_hash: IDL.Vec(IDL.Nat8),
    intent: journal_SendIntent,
    sequence: IDL.Nat64,
  })
  const journal_Result_1 = IDL.Variant({
    Ok: IDL.Opt(journal_JournalRecord),
    Err: journal_ErrorCode,
  })
  const vault_ChallengePurpose = IDL.Variant({
    Login: IDL.Null,
    Withdrawal: IDL.Null,
  })
  const vault_ChallengeRequest = IDL.Record({
    principal: IDL.Principal,
    origin: IDL.Text,
    network: vault_Network,
    purpose: vault_ChallengePurpose,
    eoa_address: IDL.Vec(IDL.Nat8),
  })
  const vault_ChallengeResponse = IDL.Record({
    typed_data: IDL.Vec(IDL.Nat8),
    nonce: IDL.Vec(IDL.Nat8),
    challenge_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
  })
  const vault_Result_15 = IDL.Variant({
    Ok: vault_ChallengeResponse,
    Err: vault_ErrorCode,
  })
  const vault_FundRequestState = IDL.Variant({
    Reserved: IDL.Null,
    Executing: IDL.Null,
    Rejected: IDL.Null,
    Accepted: IDL.Null,
    Unknown: IDL.Null,
    Settled: IDL.Null,
  })
  const vault_FundEvent = IDL.Record({
    at: IDL.Nat64,
    kind: vault_FundActionKind,
    state: vault_FundRequestState,
    event_id: IDL.Vec(IDL.Nat8),
    amount: IDL.Nat64,
  })
  const vault_Paged = IDL.Record({
    next_cursor: IDL.Opt(IDL.Vec(IDL.Nat8)),
    items: IDL.Vec(vault_FundEvent),
    revision: IDL.Nat64,
    observed_at: IDL.Nat64,
  })
  const vault_Result_17 = IDL.Variant({
    Ok: vault_Paged,
    Err: vault_ErrorCode,
  })
  const core_PrepareRecovery = IDL.Record({
    account_id: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    user_id: IDL.Vec(IDL.Nat8),
    master_address: IDL.Vec(IDL.Nat8),
  })
  const core_Result_10 = IDL.Variant({
    Ok: core_RecoveryFenceToken,
    Err: core_ErrorCode,
  })
  const vault_OpenSessionRequest = IDL.Record({
    eoa_signature: IDL.Vec(IDL.Nat8),
    challenge_id: IDL.Vec(IDL.Nat8),
  })
  const vault_Result_18 = IDL.Variant({
    Ok: vault_SessionHandle,
    Err: vault_ErrorCode,
  })
  const vault_Result_20 = IDL.Variant({
    Ok: IDL.Nat32,
    Err: vault_ErrorCode,
  })
  const journal_Result_2 = IDL.Variant({
    Ok: IDL.Vec(journal_JournalRecord),
    Err: journal_ErrorCode,
  })
  const journal_RecoveryRecord = IDL.Record({
    hash: IDL.Vec(IDL.Nat8),
    event: journal_RecoveryEvent,
    previous_hash: IDL.Vec(IDL.Nat8),
    sequence: IDL.Nat64,
  })
  const journal_Result_3 = IDL.Variant({
    Ok: IDL.Opt(journal_RecoveryRecord),
    Err: journal_ErrorCode,
  })
  const journal_Result_4 = IDL.Variant({
    Ok: IDL.Vec(journal_RecoveryRecord),
    Err: journal_ErrorCode,
  })
  const core_PreflightResolution = IDL.Variant({
    Applied: IDL.Null,
    Rejected: IDL.Null,
  })
  const vault_SessionStatus = IDL.Record({
    principal: IDL.Principal,
    user_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
    revocation_generation: IDL.Nat64,
  })
  const vault_Result_22 = IDL.Variant({
    Ok: vault_SessionStatus,
    Err: vault_ErrorCode,
  })
  const core_SweepOutcome = IDL.Record({
    reconciled: IDL.Nat32,
    dispatched: IDL.Nat32,
    cancels: IDL.Nat32,
  })
  const core_Result_13 = IDL.Variant({
    Ok: core_SweepOutcome,
    Err: core_ErrorCode,
  })
  const vault_HttpHeader = IDL.Record({
    value: IDL.Text,
    name: IDL.Text,
  })
  const vault_HttpRequestResult = IDL.Record({
    status: IDL.Nat,
    body: IDL.Vec(IDL.Nat8),
    headers: IDL.Vec(vault_HttpHeader),
  })
  const vault_TransformArgs = IDL.Record({
    context: IDL.Vec(IDL.Nat8),
    response: vault_HttpRequestResult,
  })
  const core_HttpHeader = IDL.Record({ value: IDL.Text, name: IDL.Text })
  const core_HttpRequestResult = IDL.Record({
    status: IDL.Nat,
    body: IDL.Vec(IDL.Nat8),
    headers: IDL.Vec(core_HttpHeader),
  })
  const core_TransformArgs = IDL.Record({
    context: IDL.Vec(IDL.Nat8),
    response: core_HttpRequestResult,
  })
  const vault_CyclesStatus = IDL.Record({
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
  const vault_Result_5 = IDL.Variant({
    Ok: vault_CyclesStatus,
    Err: vault_ErrorCode,
  })
  const vault_EnvironmentView = IDL.Record({
    ecdsa_key_id: IDL.Text,
    info_url: IDL.Text,
    network: vault_Network,
    exchange_url: IDL.Text,
  })
  const vault_Result_7 = IDL.Variant({
    Ok: vault_EnvironmentView,
    Err: vault_ErrorCode,
  })
  const vault_Result_12 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Bool, IDL.Bool),
    Err: vault_ErrorCode,
  })
  const vault_Result_11 = IDL.Variant({
    Ok: IDL.Opt(IDL.Principal),
    Err: vault_ErrorCode,
  })
  const vault_Result_16 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Nat64, IDL.Bool),
    Err: vault_ErrorCode,
  })
  const vault_HpkeRequest = IDL.Record({
    aad: IDL.Vec(IDL.Nat8),
    request_id: IDL.Vec(IDL.Nat8),
    method: IDL.Text,
    ciphertext: IDL.Vec(IDL.Nat8),
    key_id: IDL.Vec(IDL.Nat8),
    network: vault_Network,
    client_public_key: IDL.Vec(IDL.Nat8),
    canister: IDL.Principal,
    expires_at: IDL.Nat64,
  })
  const vault_HpkeResponse = IDL.Record({
    request_id: IDL.Vec(IDL.Nat8),
    ciphertext: IDL.Vec(IDL.Nat8),
    key_id: IDL.Vec(IDL.Nat8),
    observed_at: IDL.Nat64,
  })
  const vault_Result_19 = IDL.Variant({
    Ok: vault_HpkeResponse,
    Err: vault_ErrorCode,
  })
  const vault_Result_21 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Bool),
    Err: vault_ErrorCode,
  })
  return IDL.Service({
    abort_recovery: IDL.Func([core_RecoveryFenceToken], [core_Result], []),
    append: IDL.Func([journal_SendIntent], [journal_Result], []),
    append_recovery_event: IDL.Func([journal_RecoveryEvent], [journal_Result], []),
    application_administrator: IDL.Func([], [IDL.Principal], ['query']),
    begin_recovery_migration: IDL.Func([], [core_Result], []),
    builder_fee_mock_status: IDL.Func([vault_SessionHandle], [vault_Result], ['query']),
    caller_principal: IDL.Func([], [IDL.Principal], ['query']),
    cancel_order: IDL.Func([core_HpkeRequest], [core_Result_1], []),
    check_eligibility_account_for_core: IDL.Func(
      [IDL.Vec(IDL.Nat8), IDL.Vec(IDL.Nat8)],
      [vault_Result_1],
      [],
    ),
    check_eligibility_for_core: IDL.Func(
      [vault_SessionHandle, IDL.Vec(IDL.Nat8)],
      [vault_Result_1],
      [],
    ),
    claim_unmatched_deposit: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Vec(IDL.Nat8)], [vault_Result_1], []),
    clear_emergency_stop: IDL.Func([], [policy_Result], []),
    clear_recovery_pause: IDL.Func([], [policy_Result], []),
    commit_recovery: IDL.Func([core_RecoveryFenceToken], [core_Result], []),
    consume_rest_budget: IDL.Func([policy_RestBudgetRequest], [policy_Result], []),
    core_configure_cycles: IDL.Func([IDL.Nat, IDL.Nat], [core_Result], []),
    core_configure_market_threshold: IDL.Func([core_MarketThreshold], [core_Result], []),
    core_get_cycles_status: IDL.Func([], [core_Result_3], []),
    core_get_environment: IDL.Func([], [core_Result_4], ['query']),
    core_get_hpke_public_key: IDL.Func([], [core_Result_5], ['query']),
    core_get_journal_send_status: IDL.Func([], [core_Result_7], ['query']),
    core_get_policy_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    core_get_send_journal: IDL.Func([], [core_Result_6], ['query']),
    core_journal_restore_status: IDL.Func([], [core_Result_9], ['query']),
    core_private_call: IDL.Func([core_HpkeRequest], [core_Result_1], []),
    core_recovery_replay_pending: IDL.Func([], [core_Result_11], ['query']),
    core_recovery_stage_status: IDL.Func([], [core_Result_12], ['query']),
    core_resume_journal: IDL.Func([], [core_Result], []),
    core_rotate_hpke_key: IDL.Func([], [core_Result_5], []),
    core_set_ecdsa_key_id: IDL.Func([IDL.Text], [core_Result], []),
    core_set_venue_endpoints: IDL.Func([IDL.Text, IDL.Text], [core_Result], []),
    core_version: IDL.Func([], [IDL.Text], ['query']),
    eligibility_status: IDL.Func([vault_SessionHandle], [vault_Result_2], ['query']),
    finish_recovery: IDL.Func([core_RecoveryFenceToken], [core_Result], []),
    finish_recovery_migration: IDL.Func([], [core_Result], []),
    get_account_snapshot: IDL.Func([core_HpkeRequest], [core_Result_1], []),
    get_agent_approval: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Nat64], [vault_Result_3], ['query']),
    get_agent_status: IDL.Func([core_SessionHandle], [core_Result_2], []),
    get_balances: IDL.Func([vault_SessionHandle], [vault_Result_4], ['query']),
    get_core_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    get_eligibility_configuration: IDL.Func([], [vault_Result_6], ['query']),
    get_fund_status: IDL.Func([vault_SessionHandle], [vault_Result_8], ['query']),
    get_funding_instructions: IDL.Func([vault_SessionHandle], [vault_Result_9], ['query']),
    get_market_status: IDL.Func([IDL.Text], [core_Result_8], ['query']),
    get_order_by_request: IDL.Func([core_HpkeRequest], [core_Result_1], []),
    get_policy: IDL.Func([], [policy_Result_1], ['query']),
    get_recovery_history_verified: IDL.Func([], [vault_Result_13], ['query']),
    get_rest_budget_status: IDL.Func([], [policy_Result_2], ['query']),
    get_stop_status: IDL.Func([], [policy_StopStatus], ['query']),
    get_trading_account: IDL.Func([vault_SessionHandle], [vault_Result_14], ['query']),
    get_trading_address: IDL.Func([vault_SessionHandle], [vault_Result_10], ['query']),
    get_vault_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    head: IDL.Func([], [journal_Result], []),
    ingest_venue_deposit: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Nat64, IDL.Text], [vault_Result_13], []),
    intent_record: IDL.Func([IDL.Text, IDL.Vec(IDL.Nat8)], [journal_Result_1], []),
    issue_challenge: IDL.Func([vault_ChallengeRequest], [vault_Result_15], []),
    journal_version: IDL.Func([], [IDL.Text], ['query']),
    list_fills: IDL.Func([core_HpkeRequest], [core_Result_1], []),
    list_fund_events: IDL.Func(
      [vault_SessionHandle, IDL.Opt(IDL.Vec(IDL.Nat8)), IDL.Nat32],
      [vault_Result_17],
      ['query'],
    ),
    list_orders: IDL.Func([core_HpkeRequest], [core_Result_1], []),
    mark_recovery_unknown: IDL.Func([core_RecoveryFenceToken], [core_Result], []),
    migrate_recovery: IDL.Func([core_PrepareRecovery], [core_Result_10], []),
    open_session: IDL.Func([vault_OpenSessionRequest], [vault_Result_18], []),
    pause_for_recovery: IDL.Func([], [policy_Result], []),
    policy_configure_rest_budget: IDL.Func([policy_RestBudgetConfig], [policy_Result], []),
    policy_version: IDL.Func([], [IDL.Text], ['query']),
    prepare_recovery: IDL.Func([core_PrepareRecovery], [core_Result_10], []),
    reconcile_deposits: IDL.Func([IDL.Vec(IDL.Nat8)], [vault_Result_20], []),
    records: IDL.Func([IDL.Nat64, IDL.Nat32], [journal_Result_2], []),
    recovery_event: IDL.Func([IDL.Vec(IDL.Nat8)], [journal_Result_3], []),
    recovery_events: IDL.Func([IDL.Nat64, IDL.Nat32], [journal_Result_4], []),
    recovery_head: IDL.Func([], [journal_Result], []),
    recovery_migration_locked: IDL.Func([], [core_Result_11], ['query']),
    refresh_market: IDL.Func([], [core_Result], []),
    resolve_unknown_action: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Bool, IDL.Text], [vault_Result_1], []),
    resolve_unknown_order_preflight: IDL.Func(
      [IDL.Vec(IDL.Nat8), core_PreflightResolution],
      [core_Result],
      [],
    ),
    role_append: IDL.Func([IDL.Text, journal_SendIntent], [journal_Result], []),
    role_append_recovery_event: IDL.Func([IDL.Text, journal_RecoveryEvent], [journal_Result], []),
    role_head: IDL.Func([IDL.Text], [journal_Result], []),
    role_intent_record: IDL.Func([IDL.Text, IDL.Text, IDL.Vec(IDL.Nat8)], [journal_Result_1], []),
    role_records: IDL.Func([IDL.Text, IDL.Nat64, IDL.Nat32], [journal_Result_2], []),
    role_recovery_event: IDL.Func([IDL.Text, IDL.Vec(IDL.Nat8)], [journal_Result_3], []),
    role_recovery_events: IDL.Func([IDL.Text, IDL.Nat64, IDL.Nat32], [journal_Result_4], []),
    role_recovery_head: IDL.Func([IDL.Text], [journal_Result], []),
    session_status: IDL.Func([vault_SessionHandle], [vault_Result_22], ['query']),
    set_emergency_stop: IDL.Func([], [policy_Result], []),
    set_market_context: IDL.Func([IDL.Text, IDL.Text], [core_Result], []),
    set_meta_cache: IDL.Func([IDL.Text, IDL.Text, IDL.Text], [core_Result], []),
    set_network: IDL.Func([IDL.Text], [vault_Result_1], []),
    set_policy_version: IDL.Func([IDL.Nat64, IDL.Vec(IDL.Text)], [policy_Result], []),
    set_recovery_history_verified: IDL.Func([IDL.Bool], [vault_Result_1], []),
    sweep: IDL.Func([], [core_Result_13], []),
    transform_balance: IDL.Func([vault_TransformArgs], [vault_HttpRequestResult], ['query']),
    transform_info: IDL.Func([core_TransformArgs], [core_HttpRequestResult], ['query']),
    transform_market_info: IDL.Func([core_TransformArgs], [core_HttpRequestResult], ['query']),
    transform_open_orders: IDL.Func([core_TransformArgs], [core_HttpRequestResult], ['query']),
    vault_configure_cycles: IDL.Func([IDL.Nat, IDL.Nat], [vault_Result_1], []),
    vault_configure_eligibility: IDL.Func(
      [IDL.Nat64, IDL.Vec(IDL.Nat8), IDL.Bool],
      [vault_Result_1],
      [],
    ),
    vault_get_cycles_status: IDL.Func([], [vault_Result_5], []),
    vault_get_environment: IDL.Func([], [vault_Result_7], ['query']),
    vault_get_hpke_public_key: IDL.Func([], [vault_Result_10], ['query']),
    vault_get_journal_send_status: IDL.Func([], [vault_Result_12], ['query']),
    vault_get_policy_principal: IDL.Func([], [IDL.Opt(IDL.Principal)], ['query']),
    vault_get_send_journal: IDL.Func([], [vault_Result_11], ['query']),
    vault_journal_restore_status: IDL.Func([], [vault_Result_16], ['query']),
    vault_private_call: IDL.Func([vault_HpkeRequest], [vault_Result_19], []),
    vault_recovery_replay_pending: IDL.Func([], [vault_Result_13], ['query']),
    vault_recovery_stage_status: IDL.Func([], [vault_Result_21], ['query']),
    vault_resume_journal: IDL.Func([], [vault_Result_1], []),
    vault_rotate_hpke_key: IDL.Func([], [vault_Result_10], []),
    vault_set_ecdsa_key_id: IDL.Func([IDL.Text], [vault_Result_1], []),
    vault_set_venue_endpoints: IDL.Func([IDL.Text, IDL.Text], [vault_Result_1], []),
    vault_transform_info: IDL.Func([vault_TransformArgs], [vault_HttpRequestResult], ['query']),
    vault_version: IDL.Func([], [IDL.Text], ['query']),
    whoami: IDL.Func([core_SessionHandle], [core_Result_5], []),
  })
}
export const init = ({ IDL }) => {
  return [IDL.Principal]
}
