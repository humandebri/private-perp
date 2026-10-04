// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/private_perp.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
import type { Principal } from '@icp-sdk/core/principal'
import type { ActorMethod } from '@icp-sdk/core/agent'
import type { IDL } from '@icp-sdk/core/candid'

export interface core_AgentGeneration {
  account_id: Uint8Array | number[]
  generation: bigint
  approved_at: [] | [bigint]
  state: core_AgentState
  agent_address: Uint8Array | number[]
  expires_at: [] | [bigint]
}
export type core_AgentState =
  | { Failed: null }
  | { Active: null }
  | { Expiring: null }
  | { Approving: null }
  | { Requested: null }
  | { Revoked: null }
export interface core_AgentStatus {
  revocation_pending: boolean
  next: [] | [core_AgentGeneration]
  current: [] | [core_AgentGeneration]
  observed_at: bigint
}
export type core_BadRequestCode =
  | { NonceReused: null }
  | { UnsupportedMarket: null }
  | { TooLarge: null }
  | { NetworkMismatch: null }
  | { MalformedPayload: null }
  | { InvalidSignature: null }
  | { QuantityOutOfRange: null }
  | { ChallengeExpired: null }
  | { ExpiredIntent: null }
  | { AmountZero: null }
  | { PrecisionExceeded: null }
  | { OriginMismatch: null }
  | { ChallengeReused: null }
  | { DestinationNotAllowed: null }
  | { MissingField: null }
  | { PriceOutOfRange: null }
  | { UnsupportedAsset: null }
export interface core_CyclesStatus {
  warning: boolean
  observed_daily_burn: bigint
  balance: bigint
  refill_target: [] | [bigint]
  exit_reserve: [] | [bigint]
  configured_daily_floor: [] | [bigint]
  estimated_days: [] | [bigint]
  new_risk_stopped: boolean
  observed_at: bigint
}
export interface core_EnvironmentView {
  ecdsa_key_id: string
  info_url: string
  network: core_Network
  exchange_url: string
}
export type core_ErrorCode =
  | { Internal: { code: string } }
  | { JournalWriterBusy: null }
  | { DuplicateIgnored: { request_id: Uint8Array | number[] } }
  | { SigningQueueFull: null }
  | { NotAllowed: { code: core_NotAllowedCode } }
  | { UpstreamUnavailable: { venue: string } }
  | { UnknownPending: { action_id: Uint8Array | number[] } }
  | { ReservationConflict: null }
  | { StaleAccountState: { max_age_ms: bigint; observed_at: bigint } }
  | { IdempotencyConflict: { request_id: Uint8Array | number[] } }
  | { UpstreamRejected: { code: string; retryable: boolean } }
  | { NotEligible: { policy_version: bigint } }
  | { VenueRateLimited: { retry_after_ms: [] | [bigint] } }
  | { RiskLimitExceeded: { limit: bigint } }
  | { SessionRevoked: null }
  | { BadRequest: { code: core_BadRequestCode; detail: string } }
  | { PolicyUnavailable: null }
  | { SessionExpired: null }
  | { InsufficientFunds: { requested: bigint; available: bigint } }
  | { Unauthenticated: { reason: string } }
export interface core_HpkeRequest {
  aad: Uint8Array | number[]
  request_id: Uint8Array | number[]
  method: string
  ciphertext: Uint8Array | number[]
  key_id: Uint8Array | number[]
  network: core_Network
  client_public_key: Uint8Array | number[]
  canister: Principal
  expires_at: bigint
}
export interface core_HpkeResponse {
  request_id: Uint8Array | number[]
  ciphertext: Uint8Array | number[]
  key_id: Uint8Array | number[]
  observed_at: bigint
}
export interface core_HttpHeader {
  value: string
  name: string
}
export interface core_HttpRequestResult {
  status: bigint
  body: Uint8Array | number[]
  headers: Array<core_HttpHeader>
}
export interface core_MarketStatus {
  eligible_for_new_risk: boolean
  market: string
  reason_code: [] | [string]
  observed_at: [] | [bigint]
}
export interface core_MarketThreshold {
  max_spread_bps: number
  min_day_notional_usdc: bigint
  expected_index: number
  market: string
  min_each_side_depth_usdc: bigint
}
export type core_Network = { Mainnet: null } | { Local: null } | { Testnet: null }
export type core_NotAllowedCode =
  | { UpgradeContentMismatch: null }
  | { AssetNotAllowed: null }
  | { SessionIssuedByUnregisteredVault: null }
  | { UpgradeTooEarly: null }
  | { AccountNotOwned: null }
  | { OrderNotFound: null }
  | { CallerMismatch: null }
  | { OrderNotCancellable: null }
  | { UpgradeAlreadyExecuted: null }
  | { OperationNotAvailable: null }
  | { UpgradeNotScheduled: null }
export type core_PreflightResolution = { Applied: null } | { Rejected: null }
export interface core_PrepareRecovery {
  account_id: Uint8Array | number[]
  request_id: Uint8Array | number[]
  user_id: Uint8Array | number[]
  master_address: Uint8Array | number[]
}
export interface core_RecoveryFenceToken {
  account_id: Uint8Array | number[]
  request_id: Uint8Array | number[]
  user_id: Uint8Array | number[]
  epoch: bigint
}
export type core_Result = { Ok: null } | { Err: core_ErrorCode }
export type core_Result_1 = { Ok: core_HpkeResponse } | { Err: core_ErrorCode }
export type core_Result_10 = { Ok: core_RecoveryFenceToken } | { Err: core_ErrorCode }
export type core_Result_11 = { Ok: boolean } | { Err: core_ErrorCode }
export type core_Result_12 = { Ok: [bigint, boolean] } | { Err: core_ErrorCode }
export type core_Result_13 = { Ok: core_SweepOutcome } | { Err: core_ErrorCode }
export type core_Result_2 = { Ok: core_AgentStatus } | { Err: core_ErrorCode }
export type core_Result_3 = { Ok: core_CyclesStatus } | { Err: core_ErrorCode }
export type core_Result_4 = { Ok: core_EnvironmentView } | { Err: core_ErrorCode }
export type core_Result_5 = { Ok: Uint8Array | number[] } | { Err: core_ErrorCode }
export type core_Result_6 = { Ok: [] | [Principal] } | { Err: core_ErrorCode }
export type core_Result_7 = { Ok: [boolean, boolean] } | { Err: core_ErrorCode }
export type core_Result_8 = { Ok: core_MarketStatus } | { Err: core_ErrorCode }
export type core_Result_9 = { Ok: [bigint, bigint, boolean] } | { Err: core_ErrorCode }
export interface core_SessionHandle {
  session_id: Uint8Array | number[]
  expires_at: bigint
  vault_principal: Principal
  revocation_generation: bigint
}
export interface core_SweepOutcome {
  reconciled: number
  dispatched: number
  cancels: number
}
export interface core_TransformArgs {
  context: Uint8Array | number[]
  response: core_HttpRequestResult
}
export type journal_BadRequestCode =
  | { NonceReused: null }
  | { UnsupportedMarket: null }
  | { TooLarge: null }
  | { NetworkMismatch: null }
  | { MalformedPayload: null }
  | { InvalidSignature: null }
  | { QuantityOutOfRange: null }
  | { ChallengeExpired: null }
  | { ExpiredIntent: null }
  | { AmountZero: null }
  | { PrecisionExceeded: null }
  | { OriginMismatch: null }
  | { ChallengeReused: null }
  | { DestinationNotAllowed: null }
  | { MissingField: null }
  | { PriceOutOfRange: null }
  | { UnsupportedAsset: null }
export type journal_ErrorCode =
  | { Internal: { code: string } }
  | { JournalWriterBusy: null }
  | { DuplicateIgnored: { request_id: Uint8Array | number[] } }
  | { SigningQueueFull: null }
  | { NotAllowed: { code: journal_NotAllowedCode } }
  | { UpstreamUnavailable: { venue: string } }
  | { UnknownPending: { action_id: Uint8Array | number[] } }
  | { ReservationConflict: null }
  | { StaleAccountState: { max_age_ms: bigint; observed_at: bigint } }
  | { IdempotencyConflict: { request_id: Uint8Array | number[] } }
  | { UpstreamRejected: { code: string; retryable: boolean } }
  | { NotEligible: { policy_version: bigint } }
  | { VenueRateLimited: { retry_after_ms: [] | [bigint] } }
  | { RiskLimitExceeded: { limit: bigint } }
  | { SessionRevoked: null }
  | { BadRequest: { code: journal_BadRequestCode; detail: string } }
  | { PolicyUnavailable: null }
  | { SessionExpired: null }
  | { InsufficientFunds: { requested: bigint; available: bigint } }
  | { Unauthenticated: { reason: string } }
export interface journal_JournalHead {
  hash: Uint8Array | number[]
  sequence: bigint
}
export interface journal_JournalRecord {
  hash: Uint8Array | number[]
  previous_hash: Uint8Array | number[]
  intent: journal_SendIntent
  sequence: bigint
}
export type journal_NotAllowedCode =
  | { UpgradeContentMismatch: null }
  | { AssetNotAllowed: null }
  | { SessionIssuedByUnregisteredVault: null }
  | { UpgradeTooEarly: null }
  | { AccountNotOwned: null }
  | { OrderNotFound: null }
  | { CallerMismatch: null }
  | { OrderNotCancellable: null }
  | { UpgradeAlreadyExecuted: null }
  | { OperationNotAvailable: null }
  | { UpgradeNotScheduled: null }
export interface journal_RecoveryEvent {
  logical_id: Uint8Array | number[]
  version: number
  payload: journal_RecoveryPayload
}
export type journal_RecoveryPayload =
  | {
      CustodyAccount: {
        account_id: Uint8Array | number[]
        kind: string
        derivation_path: string
        network: string
        user_id: [] | [Uint8Array | number[]]
        address: Uint8Array | number[]
      }
    }
  | {
      OrderRisk: {
        account_id: Uint8Array | number[]
        user_id: Uint8Array | number[]
        state: string
        risk_micros: bigint
        order_id: Uint8Array | number[]
      }
    }
  | {
      WithdrawalAccepted: {
        request_id: Uint8Array | number[]
        action_id: Uint8Array | number[]
        destination: Uint8Array | number[]
        body_hash: Uint8Array | number[]
        intent_expires_at_ms: bigint
        user_id: Uint8Array | number[]
        amount_micros: bigint
        nonce: bigint
        accepted_at_ms: bigint
        reserve_account_id: Uint8Array | number[]
        intent_nonce: bigint
      }
    }
  | {
      Fill: {
        account_id: Uint8Array | number[]
        fill_id: Uint8Array | number[]
        size_micros: bigint
        order_id: Uint8Array | number[]
        price_micros: bigint
      }
    }
  | {
      RecoveryAccepted: {
        request_id: Uint8Array | number[]
        action_id: Uint8Array | number[]
        destination: Uint8Array | number[]
        body_hash: Uint8Array | number[]
        trading_account_id: Uint8Array | number[]
        user_id: Uint8Array | number[]
        amount_micros: bigint
        nonce: bigint
        accepted_at_ms: bigint
        reserve_account_id: Uint8Array | number[]
      }
    }
  | {
      OrderActionResult: {
        account_id: Uint8Array | number[]
        hl_oid: [] | [bigint]
        client_request_id: Uint8Array | number[]
        kind: string
        observed_at_ms: bigint
        filled: boolean
        order_id: Uint8Array | number[]
        accepted: boolean
      }
    }
  | {
      IdentityRegistration: {
        owner: Principal
        network: string
        user_id: Uint8Array | number[]
        eoa_address: Uint8Array | number[]
      }
    }
  | {
      Reservation: {
        account_id: Uint8Array | number[]
        request_id: Uint8Array | number[]
        user_id: Uint8Array | number[]
        amount_micros: bigint
        state: string
      }
    }
  | {
      OrderStatusObserved: {
        account_id: Uint8Array | number[]
        hl_oid: bigint
        observed_at_ms: bigint
        state: string
        evidence_digest: Uint8Array | number[]
        order_id: Uint8Array | number[]
      }
    }
  | {
      AllocationAccepted: {
        account_id: Uint8Array | number[]
        request_id: Uint8Array | number[]
        action_id: Uint8Array | number[]
        destination: Uint8Array | number[]
        body_hash: Uint8Array | number[]
        user_id: Uint8Array | number[]
        amount_micros: bigint
        nonce: bigint
        accepted_at_ms: bigint
      }
    }
  | {
      RecoverySettlement: {
        request_id: Uint8Array | number[]
        action_id: Uint8Array | number[]
        observed_at_ms: bigint
        trading_account_id: Uint8Array | number[]
        user_id: Uint8Array | number[]
        amount_micros: bigint
        evidence_digest: Uint8Array | number[]
        nonce: bigint
        accepted: boolean
      }
    }
  | {
      OrderAccepted: {
        account_id: Uint8Array | number[]
        request_id: Uint8Array | number[]
        body_hash: Uint8Array | number[]
        reduce_only: boolean
        cloid: Uint8Array | number[]
        user_id: Uint8Array | number[]
        risk_micros: bigint
        accepted_at_ms: bigint
        order_id: Uint8Array | number[]
      }
    }
  | {
      IdentityAccount: {
        account_id: Uint8Array | number[]
        owner: Principal
        user_id: Uint8Array | number[]
        address: Uint8Array | number[]
      }
    }
  | {
      FundTransferResult: {
        request_id: Uint8Array | number[]
        action_id: Uint8Array | number[]
        destination: Uint8Array | number[]
        kind: string
        observed_at_ms: bigint
        user_id: Uint8Array | number[]
        amount_micros: bigint
        evidence_digest: Uint8Array | number[]
        nonce: bigint
        source_account_id: Uint8Array | number[]
        accepted: boolean
      }
    }
  | {
      ExternalOutcome: {
        account_id: Uint8Array | number[]
        request_id: Uint8Array | number[]
        kind: string
        state: string
        evidence_digest: Uint8Array | number[]
      }
    }
  | {
      TradingBalanceObserved: {
        account_id: Uint8Array | number[]
        previous_equity: bigint
        observed_at_ms: bigint
        user_id: Uint8Array | number[]
        equity: bigint
      }
    }
  | {
      DepositClaim: {
        claimed_at_ms: bigint
        user_id: Uint8Array | number[]
        amount_micros: bigint
        event_id: Uint8Array | number[]
      }
    }
  | {
      DepositCreditWithFee: {
        fee_micros: bigint
        observed_at_ms: bigint
        network: string
        sender: [] | [Uint8Array | number[]]
        amount_micros: bigint
        address: Uint8Array | number[]
        tx_hash: Uint8Array | number[]
      }
    }
  | { Baseline: { state_digest: Uint8Array | number[] } }
  | {
      LedgerPosting: {
        account_id: Uint8Array | number[]
        user_id: Uint8Array | number[]
        amount_micros: bigint
        category: string
        posting_id: Uint8Array | number[]
      }
    }
  | {
      FillObserved: {
        fee: bigint
        tid: bigint
        account_id: Uint8Array | number[]
        hl_oid: bigint
        filled_at_ms: bigint
        user_id: Uint8Array | number[]
        quantity: string
        market: string
        order_id: Uint8Array | number[]
        price: string
      }
    }
  | {
      RecoveryPostResult: {
        request_id: Uint8Array | number[]
        action_id: Uint8Array | number[]
        observed_at_ms: bigint
        trading_account_id: Uint8Array | number[]
        user_id: Uint8Array | number[]
        amount_micros: bigint
        evidence_digest: Uint8Array | number[]
        nonce: bigint
        accepted: boolean
      }
    }
  | {
      DepositCredit: {
        fee_micros: [] | [bigint]
        observed_at_ms: bigint
        network: string
        sender: [] | [Uint8Array | number[]]
        amount_micros: bigint
        address: Uint8Array | number[]
        tx_hash: Uint8Array | number[]
      }
    }
export interface journal_RecoveryRecord {
  hash: Uint8Array | number[]
  event: journal_RecoveryEvent
  previous_hash: Uint8Array | number[]
  encoded_payload: [] | [Uint8Array | number[]]
  sequence: bigint
}
export type journal_Result = { Ok: journal_JournalHead } | { Err: journal_ErrorCode }
export type journal_Result_1 = { Ok: boolean } | { Err: journal_ErrorCode }
export type journal_Result_2 = { Ok: [] | [journal_JournalRecord] } | { Err: journal_ErrorCode }
export type journal_Result_3 = { Ok: Array<journal_JournalRecord> } | { Err: journal_ErrorCode }
export type journal_Result_4 = { Ok: [] | [journal_RecoveryRecord] } | { Err: journal_ErrorCode }
export type journal_Result_5 = { Ok: Array<journal_RecoveryRecord> } | { Err: journal_ErrorCode }
export type journal_Result_6 = { Ok: null } | { Err: journal_ErrorCode }
export interface journal_SendIntent {
  account_id: Uint8Array | number[]
  request_id: Uint8Array | number[]
  kind: string
  nonce: bigint
  digest: Uint8Array | number[]
}
export type policy_BadRequestCode =
  | { NonceReused: null }
  | { UnsupportedMarket: null }
  | { TooLarge: null }
  | { NetworkMismatch: null }
  | { MalformedPayload: null }
  | { InvalidSignature: null }
  | { QuantityOutOfRange: null }
  | { ChallengeExpired: null }
  | { ExpiredIntent: null }
  | { AmountZero: null }
  | { PrecisionExceeded: null }
  | { OriginMismatch: null }
  | { ChallengeReused: null }
  | { DestinationNotAllowed: null }
  | { MissingField: null }
  | { PriceOutOfRange: null }
  | { UnsupportedAsset: null }
export type policy_BudgetClass = { Exit: null } | { Reconcile: null } | { NewRisk: null }
export type policy_ErrorCode =
  | { Internal: { code: string } }
  | { JournalWriterBusy: null }
  | { DuplicateIgnored: { request_id: Uint8Array | number[] } }
  | { SigningQueueFull: null }
  | { NotAllowed: { code: policy_NotAllowedCode } }
  | { UpstreamUnavailable: { venue: string } }
  | { UnknownPending: { action_id: Uint8Array | number[] } }
  | { ReservationConflict: null }
  | { StaleAccountState: { max_age_ms: bigint; observed_at: bigint } }
  | { IdempotencyConflict: { request_id: Uint8Array | number[] } }
  | { UpstreamRejected: { code: string; retryable: boolean } }
  | { NotEligible: { policy_version: bigint } }
  | { VenueRateLimited: { retry_after_ms: [] | [bigint] } }
  | { RiskLimitExceeded: { limit: bigint } }
  | { SessionRevoked: null }
  | { BadRequest: { code: policy_BadRequestCode; detail: string } }
  | { PolicyUnavailable: null }
  | { SessionExpired: null }
  | { InsufficientFunds: { requested: bigint; available: bigint } }
  | { Unauthenticated: { reason: string } }
export type policy_NotAllowedCode =
  | { UpgradeContentMismatch: null }
  | { AssetNotAllowed: null }
  | { SessionIssuedByUnregisteredVault: null }
  | { UpgradeTooEarly: null }
  | { AccountNotOwned: null }
  | { OrderNotFound: null }
  | { CallerMismatch: null }
  | { OrderNotCancellable: null }
  | { UpgradeAlreadyExecuted: null }
  | { OperationNotAvailable: null }
  | { UpgradeNotScheduled: null }
export interface policy_Policy {
  markets: Array<string>
  version: bigint
}
export interface policy_RestBudgetConfig {
  exit_reserve: number
  capacity: number
}
export interface policy_RestBudgetRequest {
  request_id: Uint8Array | number[]
  weight: number
  class: policy_BudgetClass
  expires_at: bigint
}
export interface policy_RestBudgetStatus {
  new_risk_used: number
  recovery_paused: boolean
  used: number
  config: [] | [policy_RestBudgetConfig]
}
export type policy_Result = { Ok: null } | { Err: policy_ErrorCode }
export type policy_Result_1 = { Ok: policy_Policy } | { Err: policy_ErrorCode }
export type policy_Result_2 = { Ok: policy_RestBudgetStatus } | { Err: policy_ErrorCode }
export interface policy_StopStatus {
  stopped: boolean
  since: [] | [bigint]
  reason: [] | [string]
}
export type vault_AccountKind = { Reserve: null } | { Trading: null }
export type vault_ActionState =
  | { Queued: null }
  | { Signing: null }
  | { Reconciled: null }
  | { Dispatching: null }
  | { Unknown: null }
  | { Signed: null }
  | { Aborted: null }
export interface vault_AgentGeneration {
  account_id: Uint8Array | number[]
  generation: bigint
  approved_at: [] | [bigint]
  state: vault_AgentState
  agent_address: Uint8Array | number[]
  expires_at: [] | [bigint]
}
export type vault_AgentState =
  | { Failed: null }
  | { Active: null }
  | { Expiring: null }
  | { Approving: null }
  | { Requested: null }
  | { Revoked: null }
export type vault_AssetId = { Usdc: null } | { BtcPerp: null } | { EthPerp: null }
export type vault_BadRequestCode =
  | { NonceReused: null }
  | { UnsupportedMarket: null }
  | { TooLarge: null }
  | { NetworkMismatch: null }
  | { MalformedPayload: null }
  | { InvalidSignature: null }
  | { QuantityOutOfRange: null }
  | { ChallengeExpired: null }
  | { ExpiredIntent: null }
  | { AmountZero: null }
  | { PrecisionExceeded: null }
  | { OriginMismatch: null }
  | { ChallengeReused: null }
  | { DestinationNotAllowed: null }
  | { MissingField: null }
  | { PriceOutOfRange: null }
  | { UnsupportedAsset: null }
export interface vault_BuilderFeeMockStatus {
  approval_records: bigint
  builder_address: [] | [Uint8Array | number[]]
  approved: boolean
  charged_micros: bigint
  expires_at: [] | [bigint]
}
export type vault_ChallengePurpose = { Login: null } | { Withdrawal: null }
export interface vault_ChallengeRequest {
  principal: Principal
  origin: string
  network: vault_Network
  purpose: vault_ChallengePurpose
  eoa_address: Uint8Array | number[]
}
export interface vault_ChallengeResponse {
  typed_data: Uint8Array | number[]
  nonce: Uint8Array | number[]
  challenge_id: Uint8Array | number[]
  expires_at: bigint
}
export interface vault_CyclesStatus {
  warning: boolean
  observed_daily_burn: bigint
  balance: bigint
  refill_target: [] | [bigint]
  exit_reserve: [] | [bigint]
  configured_daily_floor: [] | [bigint]
  estimated_days: [] | [bigint]
  new_risk_stopped: boolean
  observed_at: bigint
}
export interface vault_EligibilityStatus {
  terms_version: bigint
  eligible: boolean
  expires_at: [] | [bigint]
}
export interface vault_EnvironmentView {
  ecdsa_key_id: string
  info_url: string
  network: vault_Network
  exchange_url: string
}
export type vault_ErrorCode =
  | { Internal: { code: string } }
  | { JournalWriterBusy: null }
  | { DuplicateIgnored: { request_id: Uint8Array | number[] } }
  | { SigningQueueFull: null }
  | { NotAllowed: { code: vault_NotAllowedCode } }
  | { UpstreamUnavailable: { venue: string } }
  | { UnknownPending: { action_id: Uint8Array | number[] } }
  | { ReservationConflict: null }
  | { StaleAccountState: { max_age_ms: bigint; observed_at: bigint } }
  | { IdempotencyConflict: { request_id: Uint8Array | number[] } }
  | { UpstreamRejected: { code: string; retryable: boolean } }
  | { NotEligible: { policy_version: bigint } }
  | { VenueRateLimited: { retry_after_ms: [] | [bigint] } }
  | { RiskLimitExceeded: { limit: bigint } }
  | { SessionRevoked: null }
  | { BadRequest: { code: vault_BadRequestCode; detail: string } }
  | { PolicyUnavailable: null }
  | { SessionExpired: null }
  | { InsufficientFunds: { requested: bigint; available: bigint } }
  | { Unauthenticated: { reason: string } }
export type vault_FundActionKind =
  | { AgentRevocation: null }
  | { SpotDeposit: null }
  | { Recovery: null }
  | { Withdrawal: null }
  | { AgentApproval: null }
  | { Allocation: null }
export interface vault_FundEvent {
  at: bigint
  kind: vault_FundActionKind
  state: vault_FundRequestState
  event_id: Uint8Array | number[]
  amount: bigint
}
export type vault_FundRequestState =
  | { Reserved: null }
  | { Executing: null }
  | { Rejected: null }
  | { Accepted: null }
  | { Unknown: null }
  | { Settled: null }
export interface vault_FundStatus {
  trading_equity: bigint
  in_transit: bigint
  unknowns: Array<vault_UnresolvedAction>
  recovery_fence: [] | [vault_RecoveryFenceStatus]
  trading_unrealized_pnl: bigint
  withdrawable: bigint
  reserve_unallocated: bigint
  reserved_for_withdrawal: bigint
  revision: bigint
  observed_at: bigint
}
export interface vault_FundingInstructions {
  source_hl_account_address: Uint8Array | number[]
  asset: vault_AssetId
  network: vault_Network
  minimum_amount: [] | [bigint]
  hl_account_address: Uint8Array | number[]
  memo_required: boolean
  account_kind: vault_AccountKind
}
export interface vault_HpkeRequest {
  aad: Uint8Array | number[]
  request_id: Uint8Array | number[]
  method: string
  ciphertext: Uint8Array | number[]
  key_id: Uint8Array | number[]
  network: vault_Network
  client_public_key: Uint8Array | number[]
  canister: Principal
  expires_at: bigint
}
export interface vault_HpkeResponse {
  request_id: Uint8Array | number[]
  ciphertext: Uint8Array | number[]
  key_id: Uint8Array | number[]
  observed_at: bigint
}
export interface vault_HttpHeader {
  value: string
  name: string
}
export interface vault_HttpRequestResult {
  status: bigint
  body: Uint8Array | number[]
  headers: Array<vault_HttpHeader>
}
export type vault_Network = { Mainnet: null } | { Local: null } | { Testnet: null }
export type vault_NotAllowedCode =
  | { UpgradeContentMismatch: null }
  | { AssetNotAllowed: null }
  | { SessionIssuedByUnregisteredVault: null }
  | { UpgradeTooEarly: null }
  | { AccountNotOwned: null }
  | { OrderNotFound: null }
  | { CallerMismatch: null }
  | { OrderNotCancellable: null }
  | { UpgradeAlreadyExecuted: null }
  | { OperationNotAvailable: null }
  | { UpgradeNotScheduled: null }
export interface vault_OpenSessionRequest {
  eoa_signature: Uint8Array | number[]
  challenge_id: Uint8Array | number[]
}
export interface vault_Paged {
  next_cursor: [] | [Uint8Array | number[]]
  items: Array<vault_FundEvent>
  revision: bigint
  observed_at: bigint
}
export type vault_RecoveryFenceStatus = { Reconciling: null } | { Preparing: null }
export type vault_Result = { Ok: vault_BuilderFeeMockStatus } | { Err: vault_ErrorCode }
export type vault_Result_1 = { Ok: null } | { Err: vault_ErrorCode }
export type vault_Result_10 = { Ok: Uint8Array | number[] } | { Err: vault_ErrorCode }
export type vault_Result_11 = { Ok: [] | [Principal] } | { Err: vault_ErrorCode }
export type vault_Result_12 = { Ok: [boolean, boolean] } | { Err: vault_ErrorCode }
export type vault_Result_13 = { Ok: boolean } | { Err: vault_ErrorCode }
export type vault_Result_14 = { Ok: [] | [Uint8Array | number[]] } | { Err: vault_ErrorCode }
export type vault_Result_15 = { Ok: vault_ChallengeResponse } | { Err: vault_ErrorCode }
export type vault_Result_16 = { Ok: [bigint, bigint, boolean] } | { Err: vault_ErrorCode }
export type vault_Result_17 = { Ok: vault_Paged } | { Err: vault_ErrorCode }
export type vault_Result_18 = { Ok: vault_SessionHandle } | { Err: vault_ErrorCode }
export type vault_Result_19 = { Ok: vault_HpkeResponse } | { Err: vault_ErrorCode }
export type vault_Result_2 = { Ok: vault_EligibilityStatus } | { Err: vault_ErrorCode }
export type vault_Result_20 = { Ok: number } | { Err: vault_ErrorCode }
export type vault_Result_21 = { Ok: [bigint, boolean] } | { Err: vault_ErrorCode }
export type vault_Result_22 = { Ok: vault_SessionStatus } | { Err: vault_ErrorCode }
export type vault_Result_3 = { Ok: [] | [vault_AgentGeneration] } | { Err: vault_ErrorCode }
export type vault_Result_4 = { Ok: [bigint, bigint] } | { Err: vault_ErrorCode }
export type vault_Result_5 = { Ok: vault_CyclesStatus } | { Err: vault_ErrorCode }
export type vault_Result_6 =
  | { Ok: [] | [[bigint, Uint8Array | number[]]] }
  | { Err: vault_ErrorCode }
export type vault_Result_7 = { Ok: vault_EnvironmentView } | { Err: vault_ErrorCode }
export type vault_Result_8 = { Ok: vault_FundStatus } | { Err: vault_ErrorCode }
export type vault_Result_9 = { Ok: vault_FundingInstructions } | { Err: vault_ErrorCode }
export interface vault_SessionHandle {
  session_id: Uint8Array | number[]
  expires_at: bigint
  vault_principal: Principal
  revocation_generation: bigint
}
export interface vault_SessionStatus {
  principal: Principal
  user_id: Uint8Array | number[]
  expires_at: bigint
  revocation_generation: bigint
}
export interface vault_TransformArgs {
  context: Uint8Array | number[]
  response: vault_HttpRequestResult
}
export interface vault_UnresolvedAction {
  action_id: Uint8Array | number[]
  kind: vault_FundActionKind
  since: bigint
  state: vault_ActionState
}
export interface _SERVICE {
  abort_recovery: ActorMethod<[core_RecoveryFenceToken], core_Result>
  append: ActorMethod<[journal_SendIntent], journal_Result>
  append_prepared: ActorMethod<[journal_SendIntent], journal_Result>
  append_recovery_event: ActorMethod<[journal_RecoveryEvent], journal_Result>
  application_administrator: ActorMethod<[], Principal>
  authorize_send: ActorMethod<[string, Uint8Array | number[]], journal_Result_1>
  begin_recovery_migration: ActorMethod<[], core_Result>
  builder_fee_mock_status: ActorMethod<[vault_SessionHandle], vault_Result>
  caller_principal: ActorMethod<[], Principal>
  cancel_order: ActorMethod<[core_HpkeRequest], core_Result_1>
  cancel_prepared_send: ActorMethod<[string, Uint8Array | number[]], journal_Result_1>
  check_eligibility_account_for_core: ActorMethod<
    [Uint8Array | number[], Uint8Array | number[]],
    vault_Result_1
  >
  check_eligibility_for_core: ActorMethod<
    [vault_SessionHandle, Uint8Array | number[]],
    vault_Result_1
  >
  clear_emergency_stop: ActorMethod<[], policy_Result>
  clear_recovery_pause: ActorMethod<[], policy_Result>
  commit_recovery: ActorMethod<[core_RecoveryFenceToken], core_Result>
  consume_rest_budget: ActorMethod<[policy_RestBudgetRequest], policy_Result>
  core_configure_cycles: ActorMethod<[bigint, bigint], core_Result>
  core_configure_market_threshold: ActorMethod<[core_MarketThreshold], core_Result>
  core_get_cycles_status: ActorMethod<[], core_Result_3>
  core_get_environment: ActorMethod<[], core_Result_4>
  core_get_hpke_public_key: ActorMethod<[], core_Result_5>
  core_get_journal_send_status: ActorMethod<[], core_Result_7>
  core_get_policy_principal: ActorMethod<[], [] | [Principal]>
  core_get_send_journal: ActorMethod<[], core_Result_6>
  core_journal_restore_status: ActorMethod<[], core_Result_9>
  core_private_call: ActorMethod<[core_HpkeRequest], core_Result_1>
  core_recovery_replay_pending: ActorMethod<[], core_Result_11>
  core_recovery_stage_status: ActorMethod<[], core_Result_12>
  core_resume_journal: ActorMethod<[], core_Result>
  core_rotate_hpke_key: ActorMethod<[], core_Result_5>
  core_set_ecdsa_key_id: ActorMethod<[string], core_Result>
  core_set_venue_endpoints: ActorMethod<[string, string], core_Result>
  core_version: ActorMethod<[], string>
  eligibility_status: ActorMethod<[vault_SessionHandle], vault_Result_2>
  finish_recovery: ActorMethod<[core_RecoveryFenceToken], core_Result>
  finish_recovery_migration: ActorMethod<[], core_Result>
  get_account_snapshot: ActorMethod<[core_HpkeRequest], core_Result_1>
  get_agent_approval: ActorMethod<[Uint8Array | number[], bigint], vault_Result_3>
  get_agent_status: ActorMethod<[core_SessionHandle], core_Result_2>
  get_balances: ActorMethod<[vault_SessionHandle], vault_Result_4>
  get_core_principal: ActorMethod<[], [] | [Principal]>
  get_eligibility_configuration: ActorMethod<[], vault_Result_6>
  get_fund_status: ActorMethod<[vault_SessionHandle], vault_Result_8>
  get_funding_instructions: ActorMethod<[vault_SessionHandle], vault_Result_9>
  get_market_status: ActorMethod<[string], core_Result_8>
  get_order_by_request: ActorMethod<[core_HpkeRequest], core_Result_1>
  get_policy: ActorMethod<[], policy_Result_1>
  get_recovery_history_verified: ActorMethod<[], vault_Result_13>
  get_rest_budget_status: ActorMethod<[], policy_Result_2>
  get_stop_status: ActorMethod<[], policy_StopStatus>
  get_trading_account: ActorMethod<[vault_SessionHandle], vault_Result_14>
  get_trading_address: ActorMethod<[vault_SessionHandle], vault_Result_10>
  get_vault_principal: ActorMethod<[], [] | [Principal]>
  head: ActorMethod<[], journal_Result>
  ingest_venue_deposit: ActorMethod<[Uint8Array | number[], bigint, string], vault_Result_13>
  intent_record: ActorMethod<[string, Uint8Array | number[]], journal_Result_2>
  issue_challenge: ActorMethod<[vault_ChallengeRequest], vault_Result_15>
  journal_version: ActorMethod<[], string>
  list_fills: ActorMethod<[core_HpkeRequest], core_Result_1>
  list_fund_events: ActorMethod<
    [vault_SessionHandle, [] | [Uint8Array | number[]], number],
    vault_Result_17
  >
  list_orders: ActorMethod<[core_HpkeRequest], core_Result_1>
  mark_recovery_unknown: ActorMethod<[core_RecoveryFenceToken], core_Result>
  migrate_recovery: ActorMethod<[core_PrepareRecovery], core_Result_10>
  open_session: ActorMethod<[vault_OpenSessionRequest], vault_Result_18>
  pause_for_recovery: ActorMethod<[], policy_Result>
  policy_configure_rest_budget: ActorMethod<[policy_RestBudgetConfig], policy_Result>
  policy_version: ActorMethod<[], string>
  prepare_recovery: ActorMethod<[core_PrepareRecovery], core_Result_10>
  queue_recovery_migration: ActorMethod<[], vault_Result_1>
  reconcile_deposits: ActorMethod<[Uint8Array | number[]], vault_Result_20>
  records: ActorMethod<[bigint, number], journal_Result_3>
  recovery_event: ActorMethod<[Uint8Array | number[]], journal_Result_4>
  recovery_events: ActorMethod<[bigint, number], journal_Result_5>
  recovery_head: ActorMethod<[], journal_Result>
  recovery_migration_locked: ActorMethod<[], core_Result_11>
  refresh_market: ActorMethod<[], core_Result>
  refresh_trading_balance: ActorMethod<[vault_SessionHandle], vault_Result_1>
  resolve_unknown_action: ActorMethod<[Uint8Array | number[], boolean, string], vault_Result_1>
  resolve_unknown_order_preflight: ActorMethod<
    [Uint8Array | number[], core_PreflightResolution],
    core_Result
  >
  role_append: ActorMethod<[string, journal_SendIntent], journal_Result>
  role_append_prepared: ActorMethod<[string, journal_SendIntent], journal_Result>
  role_append_recovery_event: ActorMethod<[string, journal_RecoveryEvent], journal_Result>
  role_authorize_send: ActorMethod<[string, string, Uint8Array | number[]], journal_Result_1>
  role_cancel_prepared_send: ActorMethod<[string, string, Uint8Array | number[]], journal_Result_1>
  role_head: ActorMethod<[string], journal_Result>
  role_intent_record: ActorMethod<[string, string, Uint8Array | number[]], journal_Result_2>
  role_records: ActorMethod<[string, bigint, number], journal_Result_3>
  role_recovery_event: ActorMethod<[string, Uint8Array | number[]], journal_Result_4>
  role_recovery_events: ActorMethod<[string, bigint, number], journal_Result_5>
  role_recovery_head: ActorMethod<[string], journal_Result>
  session_status: ActorMethod<[vault_SessionHandle], vault_Result_22>
  set_emergency_stop: ActorMethod<[], policy_Result>
  set_market_context: ActorMethod<[string, string], core_Result>
  set_meta_cache: ActorMethod<[string, string, string], core_Result>
  set_network: ActorMethod<[string], vault_Result_1>
  set_policy_version: ActorMethod<[bigint, Array<string>], policy_Result>
  set_recovery_history_verified: ActorMethod<[boolean], vault_Result_1>
  sweep: ActorMethod<[], core_Result_13>
  transform_balance: ActorMethod<[vault_TransformArgs], vault_HttpRequestResult>
  transform_info: ActorMethod<[core_TransformArgs], core_HttpRequestResult>
  transform_market_info: ActorMethod<[core_TransformArgs], core_HttpRequestResult>
  transform_open_orders: ActorMethod<[core_TransformArgs], core_HttpRequestResult>
  vault_configure_cycles: ActorMethod<[bigint, bigint], vault_Result_1>
  vault_configure_eligibility: ActorMethod<[bigint, Uint8Array | number[], boolean], vault_Result_1>
  vault_get_cycles_status: ActorMethod<[], vault_Result_5>
  vault_get_environment: ActorMethod<[], vault_Result_7>
  vault_get_hpke_public_key: ActorMethod<[], vault_Result_10>
  vault_get_journal_send_status: ActorMethod<[], vault_Result_12>
  vault_get_policy_principal: ActorMethod<[], [] | [Principal]>
  vault_get_send_journal: ActorMethod<[], vault_Result_11>
  vault_journal_restore_status: ActorMethod<[], vault_Result_16>
  vault_private_call: ActorMethod<[vault_HpkeRequest], vault_Result_19>
  vault_recovery_replay_pending: ActorMethod<[], vault_Result_13>
  vault_recovery_stage_status: ActorMethod<[], vault_Result_21>
  vault_resume_journal: ActorMethod<[], vault_Result_1>
  vault_rotate_hpke_key: ActorMethod<[], vault_Result_10>
  vault_set_ecdsa_key_id: ActorMethod<[string], vault_Result_1>
  vault_set_venue_endpoints: ActorMethod<[string, string], vault_Result_1>
  vault_transform_info: ActorMethod<[vault_TransformArgs], vault_HttpRequestResult>
  vault_version: ActorMethod<[], string>
  whoami: ActorMethod<[core_SessionHandle], core_Result_5>
}
export declare const idlFactory: IDL.InterfaceFactory
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[]
