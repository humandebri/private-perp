// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/funds_vault.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
import type { Principal } from '@icp-sdk/core/principal'
import type { ActorMethod } from '@icp-sdk/core/agent'
import type { IDL } from '@icp-sdk/core/candid'

/**
 * 口座の用途。共通保管（取引しない）とユーザー別取引口座を区別する。
 */
export type AccountKind = { Reserve: null } | { Trading: null }
/**
 * action状態（`docs/phase-0/state-machines.md` 2節）。
 */
export type ActionState =
  | { Queued: null }
  | { Signing: null }
  | { Reconciled: null }
  | { Dispatching: null }
  | { Unknown: null }
  | { Signed: null }
  | { Aborted: null }
export interface AgentGeneration {
  account_id: Uint8Array | number[]
  generation: bigint
  approved_at: [] | [bigint]
  state: AgentState
  agent_address: Uint8Array | number[]
  expires_at: [] | [bigint]
}
/**
 * Agent世代の状態（`Implementation.md` 7章）。
 */
export type AgentState =
  | { Failed: null }
  | { Active: null }
  | { Expiring: null }
  | { Approving: null }
  | { Requested: null }
  | { Revoked: null }
export interface AllocationRequest {
  client_request_id: Uint8Array | number[]
  target: AccountKind
  intent_signature: [] | [Uint8Array | number[]]
  session: SessionHandle
  amount: bigint
}
/**
 * 資産・銘柄。初期対応はHyperCore USDCとBTC・ETH perpsのみ。
 */
export type AssetId = { Usdc: null } | { BtcPerp: null } | { EthPerp: null }
/**
 * 入力不正の理由コード。
 */
export type BadRequestCode =
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
/**
 * challengeの用途。`withdrawal` のchallengeはセッション確立に使えない。
 */
export type ChallengePurpose = { Login: null } | { Withdrawal: null }
export interface ChallengeRequest {
  principal: Principal
  origin: string
  network: Network
  purpose: ChallengePurpose
  /**
   * EOAアドレス（20バイト）。
   */
  eoa_address: Uint8Array | number[]
}
export interface ChallengeResponse {
  /**
   * EIP-712 typed data（origin・network・canister・用途・nonce・期限を含む）。
   */
  typed_data: Uint8Array | number[]
  nonce: Uint8Array | number[]
  challenge_id: Uint8Array | number[]
  expires_at: bigint
}
export type Destination = { AuthenticatedEoaHlAccount: null }
/**
 * 解決済みの環境設定（既定値の適用と検証を通したもの）。
 */
export interface EnvironmentView {
  /**
   * 閾値ECDSAのkey ID。
   */
  ecdsa_key_id: string
  /**
   * Hyperliquidの`/info` endpoint。
   */
  info_url: string
  network: Network
  /**
   * Hyperliquidの`/exchange` endpoint。
   */
  exchange_url: string
}
/**
 * API全体のエラー。
 */
export type ErrorCode =
  | { Internal: { code: string } }
  | { DuplicateIgnored: { request_id: Uint8Array | number[] } }
  | { SigningQueueFull: null }
  | { NotAllowed: { code: NotAllowedCode } }
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
  | { BadRequest: { code: BadRequestCode; detail: string } }
  | { PolicyUnavailable: null }
  | { SessionExpired: null }
  | { InsufficientFunds: { requested: bigint; available: bigint } }
  | { Unauthenticated: { reason: string } }
export type FundActionKind =
  | { AgentRevocation: null }
  | { Recovery: null }
  | { Withdrawal: null }
  | { AgentApproval: null }
  | { Allocation: null }
/**
 * 資金履歴の1件。
 */
export interface FundEvent {
  at: bigint
  kind: FundActionKind
  state: FundRequestState
  event_id: Uint8Array | number[]
  amount: bigint
}
export interface FundRequestAccepted {
  request_id: Uint8Array | number[]
  accepted_at: bigint
  /**
   * 送信するactionのID。wire payloadを構築する段階（署名時）に確定するため未確定は`None`。
   */
  fund_action_id: [] | [Uint8Array | number[]]
  state: FundRequestState
}
/**
 * 資金要求の状態（`state-machines.md` 3節）。
 */
export type FundRequestState =
  | { Reserved: null }
  | { Executing: null }
  | { Rejected: null }
  | { Accepted: null }
  | { Unknown: null }
  | { Settled: null }
export interface FundStatus {
  trading_equity: bigint
  in_transit: bigint
  unknowns: Array<UnresolvedAction>
  trading_unrealized_pnl: bigint
  withdrawable: bigint
  reserve_unallocated: bigint
  reserved_for_withdrawal: bigint
  revision: bigint
  observed_at: bigint
}
export interface FundingInstructions {
  asset: AssetId
  network: Network
  /**
   * 最小額。Phase 1の実測で確定するまでは `None`。
   */
  minimum_amount: [] | [bigint]
  hl_account_address: Uint8Array | number[]
  memo_required: boolean
  account_kind: AccountKind
}
/**
 * # HTTP Header.
 *
 * Represents a HTTP header.
 *
 * See [`HttpRequestArgs::headers`] and [`HttpRequestResult::headers`].
 */
export interface HttpHeader {
  /**
   * Value of the header.
   */
  value: string
  /**
   * Name of the header.
   */
  name: string
}
/**
 * # HTTP Request Result
 *
 * Result type of [`http_request`](https://docs.internetcomputer.org/references/management-canister/#http_request).
 */
export interface HttpRequestResult {
  /**
   * The response status (e.g. 200, 404).
   */
  status: bigint
  /**
   * The response’s body.
   */
  body: Uint8Array | number[]
  /**
   * List of HTTP response headers and their corresponding values.
   */
  headers: Array<HttpHeader>
}
/**
 * IC network。`docs/phase-0/environments.md` の環境分離の単位。
 */
export type Network = { Mainnet: null } | { Local: null } | { Testnet: null }
/**
 * 権限・状態に起因する拒否の理由コード。
 */
export type NotAllowedCode =
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
export interface OpenSessionRequest {
  /**
   * 65バイトのEOA署名。
   */
  eoa_signature: Uint8Array | number[]
  challenge_id: Uint8Array | number[]
}
/**
 * カーソルページング（`docs/phase-0/api-contract.md` 1.2）。
 */
export interface Paged {
  next_cursor: [] | [Uint8Array | number[]]
  items: Array<FundEvent>
  revision: bigint
  observed_at: bigint
}
export type Result = { Ok: AgentGeneration } | { Err: ErrorCode }
export type Result_1 = { Ok: null } | { Err: ErrorCode }
export type Result_10 = { Ok: ChallengeResponse } | { Err: ErrorCode }
export type Result_11 = { Ok: Paged } | { Err: ErrorCode }
export type Result_12 = { Ok: SessionHandle } | { Err: ErrorCode }
export type Result_13 = { Ok: number } | { Err: ErrorCode }
export type Result_14 = { Ok: FundRequestAccepted } | { Err: ErrorCode }
export type Result_15 = { Ok: SessionStatus } | { Err: ErrorCode }
export type Result_2 = { Ok: boolean } | { Err: ErrorCode }
export type Result_3 = { Ok: [] | [AgentGeneration] } | { Err: ErrorCode }
export type Result_4 = { Ok: [bigint, bigint] } | { Err: ErrorCode }
export type Result_5 = { Ok: EnvironmentView } | { Err: ErrorCode }
export type Result_6 = { Ok: FundStatus } | { Err: ErrorCode }
export type Result_7 = { Ok: FundingInstructions } | { Err: ErrorCode }
export type Result_8 = { Ok: Uint8Array | number[] } | { Err: ErrorCode }
export type Result_9 = { Ok: [] | [Uint8Array | number[]] } | { Err: ErrorCode }
/**
 * 失効世代付きセッション。`trading_core` はvaultが発行したものだけを受け入れる。
 */
export interface SessionHandle {
  session_id: Uint8Array | number[]
  expires_at: bigint
  vault_principal: Principal
  revocation_generation: bigint
}
/**
 * セッションの有効性（`trading_core` がvaultへ問い合わせる）。
 *
 * 束縛されたprincipalを含むため、呼び出し側は自分の`msg_caller`と比較して認可する
 * （vault側で呼び出し元を束縛できない、canister間の検証経路のため）。
 */
export interface SessionStatus {
  principal: Principal
  user_id: Uint8Array | number[]
  expires_at: bigint
  revocation_generation: bigint
}
/**
 * # Transform Args.
 *
 * ```text
 * record {
 * response : http_response;
 * context : blob;
 * }
 * ```
 *
 * See [`TransformContext`].
 */
export interface TransformArgs {
  /**
   * Context for response transformation
   */
  context: Uint8Array | number[]
  /**
   * Raw response from remote service, to be transformed
   */
  response: HttpRequestResult
}
/**
 * 未解決のaction。出金可能額へ算入しない。
 */
export interface UnresolvedAction {
  action_id: Uint8Array | number[]
  kind: FundActionKind
  since: bigint
  state: ActionState
}
export interface WithdrawalRequest {
  /**
   * 初期の出金先は認証EOAのHL口座のみ。
   */
  destination: Destination
  asset: AssetId
  client_request_id: Uint8Array | number[]
  network: Network
  intent_signature: Uint8Array | number[]
  session: SessionHandle
  nonce: bigint
  amount: bigint
  expires_at: bigint
}
export interface _SERVICE {
  /**
   * 渡されたAgentアドレスを世代へ承認する（master鍵で署名して送信し、結果を永続化する）。
   *
   * `generation` は `trading_core` が採番した世代を渡す。承認はvaultの
   * `agent_generations` に保存され、取引所の応答が不明な場合は `active` にしない。
   */
  approve_agent_generation: ActorMethod<[SessionHandle, bigint, Uint8Array | number[]], Result>
  /**
   * 呼び出し元のPrincipal（診断用。認可の判断は各メソッド内で行う）。
   */
  caller_principal: ActorMethod<[], Principal>
  /**
   * 宛先が未解決だった入金を、後から判明した利用者へ振り替える（controllerのみ）。
   *
   * `credit` がsuspenseへ計上したイベントだけを対象にする（既に本人へ計上済みの
   * イベントを再計上しない）。同一イベントの二重請求は仕訳の要求IDで拒否する。
   */
  claim_unmatched_deposit: ActorMethod<[Uint8Array | number[], Uint8Array | number[]], Result_1>
  /**
   * 取引所の入金を本人へ計上する（controllerのみ。宛先が導出口座の場合）。
   */
  credit_venue_deposit: ActorMethod<
    [Uint8Array | number[], bigint, Uint8Array | number[], string],
    Result_2
  >
  /**
   * 口座・世代の承認状態（`trading_core` が状態表示と署名可否の判断に使う）。
   */
  get_agent_approval: ActorMethod<[Uint8Array | number[], bigint], Result_3>
  /**
   * 本人の残高（`trading_core` がsnapshotを作るための参照）。
   *
   * 戻り値は `(取引口座の残高, 出金可能額)`。認可のcaller束縛は呼び出し側（core）が
   * `session_status` で行う。
   */
  get_balances: ActorMethod<[SessionHandle], Result_4>
  /**
   * 現在の環境設定（診断用・公開）。秘密は含まない。
   */
  get_environment: ActorMethod<[], Result_5>
  /**
   * 資金状態（認証済みセッションが必要）。
   */
  get_fund_status: ActorMethod<[SessionHandle], Result_6>
  /**
   * 入金案内（認証済みセッションが必要）。
   */
  get_funding_instructions: ActorMethod<[SessionHandle], Result_7>
  /**
   * 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
   */
  get_hpke_public_key: ActorMethod<[], Result_8>
  /**
   * 本人の取引口座ID（`trading_core` が所有権の確認に使う）。
   */
  get_trading_account: ActorMethod<[SessionHandle], Result_9>
  /**
   * 本人の取引口座アドレス（着金確認や照合に使う）。
   */
  get_trading_address: ActorMethod<[SessionHandle], Result_8>
  /**
   * 取引所の入金（ledger update）を記録する（controllerのみ）。
   *
   * 正規化したイベントID（`keccak256("deposit" ‖ tx_hash)`）で**二重計上を防ぐ**。
   * 本番ではreplicatedな`/info`照合がこの経路を呼ぶ。ユーザーへの紐付け（宛先アドレス→
   * 利用者）と`deposit_confirmed`の起票は次段階（アドレス写像の実装後）に行う。
   */
  ingest_venue_deposit: ActorMethod<[Uint8Array | number[], bigint, string], Result_2>
  /**
   * ログインchallengeを発行する。
   */
  issue_challenge: ActorMethod<[ChallengeRequest], Result_10>
  /**
   * 資金履歴（認証済みセッションが必要）。
   */
  list_fund_events: ActorMethod<[SessionHandle, [] | [Uint8Array | number[]], number], Result_11>
  /**
   * challengeを消費してセッションを発行する。
   */
  open_session: ActorMethod<[OpenSessionRequest], Result_12>
  /**
   * 入金先（準備口座）を用意する。`get_funding_instructions` の前提を作る。
   */
  provision_reserve_account: ActorMethod<[SessionHandle], Result_8>
  /**
   * 取引所の入金を取得して取り込む（controllerのみ）。
   *
   * 取得はreplicated outcall（変換関数で決定論化）、取り込みは検証済みの`deposits::credit`。
   */
  reconcile_deposits: ActorMethod<[Uint8Array | number[]], Result_13>
  /**
   * 配分を要求する（受付＋予約）。
   */
  request_allocation: ActorMethod<[AllocationRequest], Result_14>
  /**
   * 回収（trading口座→準備口座）を要求する。
   */
  request_recovery: ActorMethod<[SessionHandle, Uint8Array | number[], bigint], Result_14>
  /**
   * 出金を要求する（本人署名の検証＋受付＋予約）。
   */
  request_withdrawal: ActorMethod<[WithdrawalRequest], Result_14>
  /**
   * 不明なactionを「未実行」として解消する（controllerのみ）。
   *
   * 取引所が実行済みと確認できた場合の消込は、証跡（tx）を伴う別経路で行うため
   * ここでは受け付けない（`OperationNotAvailable`）。`unknown` と `dispatching` の
   * どちらも対象にするが、**取引所へ照会した証跡**を `evidence` として必須にする。
   */
  resolve_unknown_action: ActorMethod<[Uint8Array | number[], boolean, string], Result_1>
  /**
   * セッションを失効させる。
   */
  revoke_session: ActorMethod<[SessionHandle], Result_1>
  /**
   * HPKEの鍵世代を更新する（controllerのみ）。
   *
   * 秘密鍵はcanister内のDBに留め、公開鍵のみを配布する（`Plan.md` 16.5）。
   */
  rotate_hpke_key: ActorMethod<[], Result_8>
  /**
   * セッションの有効性（canister間の検証経路。呼び出し元は返却されたprincipalを検証する）。
   */
  session_status: ActorMethod<[SessionHandle], Result_15>
  /**
   * 閾値ECDSAのkey IDを設定する（controllerのみ）。
   *
   * testnetの鍵名はデプロイ後に実測して確定する（`docs/phase-0/environments.md` 2節）。
   */
  set_ecdsa_key_id: ActorMethod<[string], Result_1>
  /**
   * 環境のnetworkを設定する（controllerのみ）。
   *
   * mainnetはPhase 2では拒否する（`docs/phase-0/environments.md` E-2）。
   */
  set_network: ActorMethod<[string], Result_1>
  /**
   * Hyperliquidのendpointを設定する（controllerのみ）。
   *
   * 設定済みのnetworkと整合しないhost（例：testnet設定にmainnet endpoint）は拒否する。
   */
  set_venue_endpoints: ActorMethod<[string, string], Result_1>
  /**
   * 変換関数：必要な要素だけを決定論的に残す（順序・付随フィールドの揺れを除く）。
   */
  transform_info: ActorMethod<[TransformArgs], HttpRequestResult>
  /**
   * このビルドのバージョン。デプロイ確認用。
   */
  version: ActorMethod<[], string>
}
export declare const idlFactory: IDL.InterfaceFactory
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[]
