// 生成物: `bash scripts/generate-frontend-bindings.sh`（元: candid/trading_core.did）
// 手で編集しない。契約（`crates/api-types`）を変えたら .did と本ファイルを再生成する。
import type { Principal } from '@icp-sdk/core/principal'
import type { ActorMethod } from '@icp-sdk/core/agent'
import type { IDL } from '@icp-sdk/core/candid'

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
export interface AgentStatus {
  revocation_pending: boolean
  next: [] | [AgentGeneration]
  current: [] | [AgentGeneration]
  observed_at: bigint
}
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
 * `close_all` の結果（建玉ごとの受付結果と、受付できなかった銘柄）。
 */
export interface CloseAllOutcome {
  submitted: Array<SubmitOrderResult>
  failed: Array<CloseFailure>
}
/**
 * 決済できなかった銘柄とその理由。
 */
export interface CloseFailure {
  error: ErrorCode
  market: string
}
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
/**
 * 個人APIの要求封筒。
 */
export interface HpkeRequest {
  /**
   * `network`・`canister`・`method`・`caller`・`request_id`・期限を束縛する。
   */
  aad: Uint8Array | number[]
  /**
   * 要求の単回使用ID（再送は拒否する）。
   */
  request_id: Uint8Array | number[]
  method: string
  /**
   * ChaCha20-Poly1305の暗号文（`enc || ciphertext`）。
   */
  ciphertext: Uint8Array | number[]
  /**
   * サーバ公開鍵のID（現行世代の公開鍵そのもの）。
   */
  key_id: Uint8Array | number[]
  network: Network
  /**
   * 応答を暗号化するブラウザ公開鍵。
   */
  client_public_key: Uint8Array | number[]
  /**
   * 呼び出し先のcanister principal（別canisterへの転用を拒否する）。
   */
  canister: Principal
  expires_at: bigint
}
/**
 * 個人APIの応答封筒。
 */
export interface HpkeResponse {
  request_id: Uint8Array | number[]
  /**
   * `client_public_key`宛の暗号文（`enc || ciphertext`）。
   */
  ciphertext: Uint8Array | number[]
  key_id: Uint8Array | number[]
  observed_at: bigint
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
/**
 * 注文種別。Marketはスリッページ上限付きIOC指値として構築する。
 */
export type OrderKind = { LimitGtc: null } | { MarketIoc: null }
/**
 * 送信結果が不明なleverage preflightを外部確認後に解決する。
 */
export type PreflightResolution = { Applied: null } | { Rejected: null }
export type Result = { Ok: bigint } | { Err: ErrorCode }
export type Result_1 = { Ok: HpkeResponse } | { Err: ErrorCode }
export type Result_2 = { Ok: CloseAllOutcome } | { Err: ErrorCode }
export type Result_3 = { Ok: SubmitOrderResult } | { Err: ErrorCode }
export type Result_4 = { Ok: AgentStatus } | { Err: ErrorCode }
export type Result_5 = { Ok: EnvironmentView } | { Err: ErrorCode }
export type Result_6 = { Ok: Uint8Array | number[] } | { Err: ErrorCode }
export type Result_7 = { Ok: AgentGeneration } | { Err: ErrorCode }
export type Result_8 = { Ok: null } | { Err: ErrorCode }
export type Result_9 = { Ok: SweepOutcome } | { Err: ErrorCode }
/**
 * 失効世代付きセッション。`trading_core` はvaultが発行したものだけを受け入れる。
 */
export interface SessionHandle {
  session_id: Uint8Array | number[]
  expires_at: bigint
  vault_principal: Principal
  revocation_generation: bigint
}
export type Side = { Buy: null } | { Sell: null }
export interface SubmitOrderArgs {
  account_id: Uint8Array | number[]
  /**
   * Marketはスリッページ上限付きIOC指値のため必須。Limitでは上限価格。
   */
  limit_price: [] | [string]
  trigger: [] | [Trigger]
  leverage: [] | [number]
  client_request_id: Uint8Array | number[]
  reduce_only: boolean
  kind: OrderKind
  side: Side
  slippage_tolerance_bps: [] | [number]
  session: SessionHandle
  /**
   * 正規化十進文字列。
   */
  quantity: string
  market: string
  expires_after: [] | [bigint]
}
export interface SubmitOrderResult {
  request_id: Uint8Array | number[]
  cloid: Uint8Array | number[]
  accepted_at: bigint
  order_id: Uint8Array | number[]
}
/**
 * `sweep`の結果（送信・取消・照合の件数）。失敗の内訳はログ・DBの状態で確認する。
 */
export interface SweepOutcome {
  /**
   * 照合した口座の件数。
   */
  reconciled: number
  /**
   * 送信した注文の件数（受理・拒否・不明を含む）。
   */
  dispatched: number
  /**
   * 送信した取消の件数。
   */
  cancels: number
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
 * 建玉単位のSL/TP（HL `positionTpsl`、reduce-only）。
 */
export interface Trigger {
  kind: TriggerKind
  is_market: boolean
  trigger_price: string
}
export type TriggerKind = { TakeProfit: null } | { StopLoss: null }
export interface _SERVICE {
  /**
   * 未終端の注文すべてに取消要求を付ける（送信はsweepが行う）。
   */
  cancel_all: ActorMethod<[SessionHandle], Result>
  /**
   * 注文の取消を要求する（**封筒必須**。署名・送信はパイプラインが行う）。
   *
   * 認証は封筒の`aad`と本文のセッションで行う（`api-contract.md` 6節）。
   */
  cancel_order: ActorMethod<[HpkeRequest], Result_1>
  /**
   * 建玉をすべて閉じる（建玉ごとに`close_position`と同じ反対売買を送る）。
   *
   * 1件の失敗で全体を止めない（建玉ごとの結果を返す）。受付IDは
   * `client_request_id`と銘柄から導出するため、同じIDの再送は同じ注文として扱われる
   * （建玉が変わっている場合は`IdempotencyConflict`になる）。
   */
  close_all: ActorMethod<[SessionHandle, Uint8Array | number[]], Result_2>
  /**
   * 建玉を閉じる（全量または比率指定）。反対売買のreduce-only IOC指値として受付ける。
   *
   * `limit_price`はスリッページ上限（公開市況から画面が決める）。省略時は観測した
   * 建玉からmark価格を近似して`DEFAULT_SLIPPAGE_BPS`の幅を付ける。
   * `ratio_bps`は建玉に対する比率（10000 = 全量）。
   */
  close_position: ActorMethod<
    [SessionHandle, Uint8Array | number[], string, number, [] | [string]],
    Result_3
  >
  /**
   * 口座snapshot（残高はvault、注文はcore。**封筒必須**）。
   */
  get_account_snapshot: ActorMethod<[HpkeRequest], Result_1>
  /**
   * Agent世代の状態（承認済みは`current`、要求中は`next`）。
   *
   * 認可にvaultへのinter-canister呼び出しが必要なためqueryにはできない（updateで提供）。
   */
  get_agent_status: ActorMethod<[SessionHandle], Result_4>
  /**
   * 現在の環境設定（診断用・公開）。秘密は含まない。
   */
  get_environment: ActorMethod<[], Result_5>
  /**
   * 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
   */
  get_hpke_public_key: ActorMethod<[], Result_6>
  /**
   * 受付結果を再送せずに照合する。不存在と他人の要求は区別しない。
   */
  get_order_by_request: ActorMethod<[HpkeRequest], Result_1>
  /**
   * 政策Canisterのprincipal（診断用）。
   */
  get_policy_principal: ActorMethod<[], [] | [Principal]>
  /**
   * vaultのprincipal（診断用）。
   */
  get_vault_principal: ActorMethod<[], [] | [Principal]>
  /**
   * 約定一覧（新しい順。**封筒必須**。認可にvaultへの問い合わせが必要なためupdate）。
   */
  list_fills: ActorMethod<[HpkeRequest], Result_1>
  /**
   * 注文一覧（新しい順。**封筒必須**）。
   *
   * **updateである理由**：認可に `funds_vault` へのinter-canister呼び出しが必要だが、
   * queryでは他Canisterを呼べない。最終設計では、(a) 個人向け読み取りをupdateのまま
   * 提供する、(b) vaultからセッション写像をcoreへ同期してqueryで返す、のいずれかを選ぶ
   * （`docs/phase-1/README.md` の残課題）。
   */
  list_orders: ActorMethod<[HpkeRequest], Result_1>
  /**
   * Agent世代を要求する（**coreが鍵を導出・保管**し、vaultはmaster署名でアドレスを承認する）。
   *
   * 未承認の世代があるうちは同じ世代を返す。注文はこの世代の鍵で署名する。
   */
  request_agent_generation: ActorMethod<[SessionHandle], Result_7>
  /**
   * 外部確認済みのleverage preflight不明状態をcontrollerが解決する。
   */
  resolve_unknown_order_preflight: ActorMethod<
    [Uint8Array | number[], PreflightResolution],
    Result_8
  >
  /**
   * HPKEの鍵世代を更新する（controllerのみ）。
   *
   * 秘密鍵はcanister内のDBに留め、公開鍵のみを配布する（`Plan.md` 16.5、
   * `docs/phase-0/api-contract.md` 6節）。更新すると以前の世代は退役し、
   * 旧鍵で作られた封筒は復号できない（クライアントは公開鍵を取得し直す）。
   */
  rotate_hpke_key: ActorMethod<[], Result_6>
  /**
   * 閾値ECDSAのkey IDを設定する（controllerのみ）。
   *
   * testnetの鍵名はデプロイ後に実測して確定する（`docs/phase-0/environments.md` 2節）。
   */
  set_ecdsa_key_id: ActorMethod<[string], Result_8>
  /**
   * 銘柄解決に使うnetwork・dexを設定する（controllerのみ）。
   */
  set_market_context: ActorMethod<[string, string], Result_8>
  /**
   * `meta`の`universe`を登録する（ローカルのブートストラップ。本番はHL `/info` から取得する）。
   */
  set_meta_cache: ActorMethod<[string, string, string], Result_8>
  /**
   * 政策Canisterのprincipalを設定する（controllerのみ）。
   */
  set_policy_principal: ActorMethod<[Principal], Result_8>
  /**
   * vaultのprincipalを設定する（controllerのみ）。
   */
  set_vault_principal: ActorMethod<[Principal], Result_8>
  /**
   * Hyperliquidのendpointを設定する（controllerのみ）。
   *
   * 設定済みのnetworkと整合しないhost（例：testnet設定にmainnet endpoint）は拒否する。
   */
  set_venue_endpoints: ActorMethod<[string, string], Result_8>
  /**
   * 受付を1件処理する（認可・検証・冪等性・pending注文の登録）。
   *
   * 署名・送信・照合はパイプライン（次段階）が行う。ここでは受付だけを確定させる。
   */
  submit_order: ActorMethod<[SessionHandle, SubmitOrderArgs], Result_3>
  /**
   * 未処理の注文・取消を送信し、取引所状態を照合する（controllerのみ）。
   *
   * 本番は `heartbeat` が間隔を空けて呼ぶ。停止した場合の手動実行の入口でもある。
   */
  sweep: ActorMethod<[], Result_9>
  /**
   * 変換関数：`/info`の応答から照合に使う要素だけを決定論的に残す。
   *
   * 全ノードで同じ本文にするため、付随フィールドと並びの揺れを落とす。解釈できない
   * 応答は空本文にし、呼び出し側は「未観測」として扱う（誤って建玉0にしない）。
   */
  transform_info: ActorMethod<[TransformArgs], HttpRequestResult>
  /**
   * このビルドのバージョン。デプロイ確認用。
   */
  version: ActorMethod<[], string>
  /**
   * セッションを検証し、本人のuser_idを返す（認可境界の試験用）。///
   * vaultに問い合わせ、返却されたprincipalが「このメッセージのcaller」と一致する場合だけ
   * user_idを返す。順序を逆にしない（callerを信用しない）。
   */
  whoami: ActorMethod<[SessionHandle], Result_6>
}
export declare const idlFactory: IDL.InterfaceFactory
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[]
