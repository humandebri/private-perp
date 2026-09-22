// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/funds_vault.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
export const idlFactory = ({ IDL }) => {
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
  const Result = IDL.Variant({ Ok: AgentGeneration, Err: ErrorCode })
  const Result_1 = IDL.Variant({ Ok: IDL.Null, Err: ErrorCode })
  const Result_2 = IDL.Variant({ Ok: IDL.Bool, Err: ErrorCode })
  const Result_3 = IDL.Variant({
    Ok: IDL.Opt(AgentGeneration),
    Err: ErrorCode,
  })
  const Result_4 = IDL.Variant({
    Ok: IDL.Tuple(IDL.Nat64, IDL.Nat64),
    Err: ErrorCode,
  })
  const Network = IDL.Variant({
    Mainnet: IDL.Null,
    Local: IDL.Null,
    Testnet: IDL.Null,
  })
  const EnvironmentView = IDL.Record({
    ecdsa_key_id: IDL.Text,
    info_url: IDL.Text,
    network: Network,
    exchange_url: IDL.Text,
  })
  const Result_5 = IDL.Variant({ Ok: EnvironmentView, Err: ErrorCode })
  const FundActionKind = IDL.Variant({
    AgentRevocation: IDL.Null,
    Recovery: IDL.Null,
    Withdrawal: IDL.Null,
    AgentApproval: IDL.Null,
    Allocation: IDL.Null,
  })
  const ActionState = IDL.Variant({
    Queued: IDL.Null,
    Signing: IDL.Null,
    Reconciled: IDL.Null,
    Dispatching: IDL.Null,
    Unknown: IDL.Null,
    Signed: IDL.Null,
    Aborted: IDL.Null,
  })
  const UnresolvedAction = IDL.Record({
    action_id: IDL.Vec(IDL.Nat8),
    kind: FundActionKind,
    since: IDL.Nat64,
    state: ActionState,
  })
  const FundStatus = IDL.Record({
    trading_equity: IDL.Nat64,
    in_transit: IDL.Nat64,
    unknowns: IDL.Vec(UnresolvedAction),
    trading_unrealized_pnl: IDL.Int64,
    withdrawable: IDL.Nat64,
    reserve_unallocated: IDL.Nat64,
    reserved_for_withdrawal: IDL.Nat64,
    revision: IDL.Nat64,
    observed_at: IDL.Nat64,
  })
  const Result_6 = IDL.Variant({ Ok: FundStatus, Err: ErrorCode })
  const AssetId = IDL.Variant({
    Usdc: IDL.Null,
    BtcPerp: IDL.Null,
    EthPerp: IDL.Null,
  })
  const AccountKind = IDL.Variant({
    Reserve: IDL.Null,
    Trading: IDL.Null,
  })
  const FundingInstructions = IDL.Record({
    asset: AssetId,
    network: Network,
    minimum_amount: IDL.Opt(IDL.Nat64),
    hl_account_address: IDL.Vec(IDL.Nat8),
    memo_required: IDL.Bool,
    account_kind: AccountKind,
  })
  const Result_7 = IDL.Variant({
    Ok: FundingInstructions,
    Err: ErrorCode,
  })
  const Result_8 = IDL.Variant({ Ok: IDL.Vec(IDL.Nat8), Err: ErrorCode })
  const Result_9 = IDL.Variant({
    Ok: IDL.Opt(IDL.Vec(IDL.Nat8)),
    Err: ErrorCode,
  })
  const ChallengePurpose = IDL.Variant({
    Login: IDL.Null,
    Withdrawal: IDL.Null,
  })
  const ChallengeRequest = IDL.Record({
    principal: IDL.Principal,
    origin: IDL.Text,
    network: Network,
    purpose: ChallengePurpose,
    eoa_address: IDL.Vec(IDL.Nat8),
  })
  const ChallengeResponse = IDL.Record({
    typed_data: IDL.Vec(IDL.Nat8),
    nonce: IDL.Vec(IDL.Nat8),
    challenge_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
  })
  const Result_10 = IDL.Variant({
    Ok: ChallengeResponse,
    Err: ErrorCode,
  })
  const FundRequestState = IDL.Variant({
    Reserved: IDL.Null,
    Executing: IDL.Null,
    Rejected: IDL.Null,
    Accepted: IDL.Null,
    Unknown: IDL.Null,
    Settled: IDL.Null,
  })
  const FundEvent = IDL.Record({
    at: IDL.Nat64,
    kind: FundActionKind,
    state: FundRequestState,
    event_id: IDL.Vec(IDL.Nat8),
    amount: IDL.Nat64,
  })
  const Paged = IDL.Record({
    next_cursor: IDL.Opt(IDL.Vec(IDL.Nat8)),
    items: IDL.Vec(FundEvent),
    revision: IDL.Nat64,
    observed_at: IDL.Nat64,
  })
  const Result_11 = IDL.Variant({ Ok: Paged, Err: ErrorCode })
  const OpenSessionRequest = IDL.Record({
    eoa_signature: IDL.Vec(IDL.Nat8),
    challenge_id: IDL.Vec(IDL.Nat8),
  })
  const Result_12 = IDL.Variant({ Ok: SessionHandle, Err: ErrorCode })
  const Result_13 = IDL.Variant({ Ok: IDL.Nat32, Err: ErrorCode })
  const AllocationRequest = IDL.Record({
    client_request_id: IDL.Vec(IDL.Nat8),
    target: AccountKind,
    intent_signature: IDL.Opt(IDL.Vec(IDL.Nat8)),
    session: SessionHandle,
    amount: IDL.Nat64,
  })
  const FundRequestAccepted = IDL.Record({
    request_id: IDL.Vec(IDL.Nat8),
    accepted_at: IDL.Nat64,
    fund_action_id: IDL.Opt(IDL.Vec(IDL.Nat8)),
    state: FundRequestState,
  })
  const Result_14 = IDL.Variant({
    Ok: FundRequestAccepted,
    Err: ErrorCode,
  })
  const Destination = IDL.Variant({ AuthenticatedEoaHlAccount: IDL.Null })
  const WithdrawalRequest = IDL.Record({
    destination: Destination,
    asset: AssetId,
    client_request_id: IDL.Vec(IDL.Nat8),
    network: Network,
    intent_signature: IDL.Vec(IDL.Nat8),
    session: SessionHandle,
    nonce: IDL.Nat64,
    amount: IDL.Nat64,
    expires_at: IDL.Nat64,
  })
  const SessionStatus = IDL.Record({
    principal: IDL.Principal,
    user_id: IDL.Vec(IDL.Nat8),
    expires_at: IDL.Nat64,
    revocation_generation: IDL.Nat64,
  })
  const Result_15 = IDL.Variant({ Ok: SessionStatus, Err: ErrorCode })
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
    approve_agent_generation: IDL.Func([SessionHandle, IDL.Nat64, IDL.Vec(IDL.Nat8)], [Result], []),
    caller_principal: IDL.Func([], [IDL.Principal], ['query']),
    claim_unmatched_deposit: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Vec(IDL.Nat8)], [Result_1], []),
    credit_venue_deposit: IDL.Func(
      [IDL.Vec(IDL.Nat8), IDL.Nat64, IDL.Vec(IDL.Nat8), IDL.Text],
      [Result_2],
      [],
    ),
    get_agent_approval: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Nat64], [Result_3], ['query']),
    get_balances: IDL.Func([SessionHandle], [Result_4], ['query']),
    get_environment: IDL.Func([], [Result_5], ['query']),
    get_fund_status: IDL.Func([SessionHandle], [Result_6], ['query']),
    get_funding_instructions: IDL.Func([SessionHandle], [Result_7], ['query']),
    get_hpke_public_key: IDL.Func([], [Result_8], ['query']),
    get_trading_account: IDL.Func([SessionHandle], [Result_9], ['query']),
    get_trading_address: IDL.Func([SessionHandle], [Result_8], ['query']),
    ingest_venue_deposit: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Nat64, IDL.Text], [Result_2], []),
    issue_challenge: IDL.Func([ChallengeRequest], [Result_10], []),
    list_fund_events: IDL.Func(
      [SessionHandle, IDL.Opt(IDL.Vec(IDL.Nat8)), IDL.Nat32],
      [Result_11],
      ['query'],
    ),
    open_session: IDL.Func([OpenSessionRequest], [Result_12], []),
    provision_reserve_account: IDL.Func([SessionHandle], [Result_8], []),
    reconcile_deposits: IDL.Func([IDL.Vec(IDL.Nat8)], [Result_13], []),
    request_allocation: IDL.Func([AllocationRequest], [Result_14], []),
    request_recovery: IDL.Func([SessionHandle, IDL.Vec(IDL.Nat8), IDL.Nat64], [Result_14], []),
    request_withdrawal: IDL.Func([WithdrawalRequest], [Result_14], []),
    resolve_unknown_action: IDL.Func([IDL.Vec(IDL.Nat8), IDL.Bool, IDL.Text], [Result_1], []),
    revoke_session: IDL.Func([SessionHandle], [Result_1], []),
    rotate_hpke_key: IDL.Func([], [Result_8], []),
    session_status: IDL.Func([SessionHandle], [Result_15], ['query']),
    set_ecdsa_key_id: IDL.Func([IDL.Text], [Result_1], []),
    set_network: IDL.Func([IDL.Text], [Result_1], []),
    set_venue_endpoints: IDL.Func([IDL.Text, IDL.Text], [Result_1], []),
    transform_info: IDL.Func([TransformArgs], [HttpRequestResult], ['query']),
    version: IDL.Func([], [IDL.Text], ['query']),
  })
}
export const init = ({ IDL }) => {
  return []
}
