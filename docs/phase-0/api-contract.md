# API契約：認証・注文・資金・変更予約

- 根拠：`Implementation.md` 2.3、5章、7章、11章、14.3、`Plan.md` 16.1〜16.5
- 状態：設計契約。実装から抽出したCandidは`candid/`に固定する。testnet・mainnetのデプロイ契約は未確定

## 1. 適用範囲と共通規約

- すべての本人向けAPIは、失効世代付きセッション（`SessionHandle`）とHPKE暗号化封筒（5節）を要求する。queryでも本人認可を省かない。
- 入力の `caller` 相当フィールドは信用しない。callerはICメッセージから取得する。
- 各updateは同期トランザクションで受付を確定してから応答する。受付はHLでの受理・約定を意味しない（`state-machines.md` 2節）。
- 外部呼び出し（`sign_with_ecdsa`・HTTPS outcall・`/info`照合）をトランザクション内で跨がない。`Implementation.md` 2.3の順序に従う。
- 応答に他ユーザーのデータ、平文の注文本文、署名対象payload、HPKE秘密鍵を含めない。
- 数値はすべて整数表現（`money-and-units.md`）。浮動小数点と指数表記を受け付けない。

### 1.1 共通型

```candid
type Blob32 = blob;              // 32バイト
type Cloid = blob;               // 16バイト固定
type RequestId = blob;           // クライアント生成。8〜32バイト
type Timestamp = nat64;          // ミリ秒。Canister時刻
type Network = variant { local; testnet; mainnet };
type AccountKind = variant { reserve; trading };
type AssetId = variant { usdc; btc_perp; eth_perp };

type SessionHandle = record {
  session_id : Blob32;
  vault_principal : principal;
  expires_at : Timestamp;
  revocation_generation : nat64;
};

type Paged<T> = record {
  items : vec T;
  next_cursor : opt blob;        // null で終端
  observed_at : Timestamp;
  revision : nat64;
};
```

- `expires_at` は30分（`Plan.md` 16.1）。更新は`open_session`の再実行ではなく再認証で行う。
- `revision` は口座・資金状態の単調増加番号である。UIは`revision`が小さい応答で表示を上書きしない（`ui-spec.md` 5節）。

### 1.2 上限（初期値）

| 項目 | 値 | 根拠 |
|---|---|---|
| 受理前payload | 16 KiB | `Implementation.md` 14.3 |
| 口座あたり未送信注文 | 100件 | 同上 |
| 口座あたり同時資金移動 | 1件 | 同上 |
| 一覧の1ページ件数 | 100件（カーソル必須） | 14.3、4.5の無制限走査禁止 |
| バッチ内注文数・payload | 実装時に上限を固定しDoS試験で調整 | 2.3、5.5 |
| レート制限 | **未確定**（Phase 1でREST weight予算と同時に確定） | 11章 |

## 2. funds_vault

本人認可（`SessionHandle`）または登録済みCanister callerを要求する。

| # | メソッド | 種別 | caller要件 | 冪等性キー |
|---|---|---|---|---|
| 1 | `issue_challenge` | update | anonymous | `nonce`（一回性） |
| 2 | `open_session` | update | EOA署名付きchallenge | `challenge_id` |
| 3 | `revoke_session` | update | 本人セッション | なし（冪等） |
| 4 | `get_funding_instructions` | query | 本人セッション | なし |
| 5 | `request_allocation` | update | 本人セッション | `client_request_id` |
| 6 | `request_withdrawal` | update | 本人セッション＋EOA intent署名 | `client_request_id` |
| 7 | `get_fund_status` | query | 本人セッション | なし |
| 8 | `list_fund_events` | query | 本人セッション | なし（カーソル） |
| 9 | `request_agent_generation` | update | 本人セッション | `client_request_id` |
| 10 | `get_agent_status` | query | 本人セッション | なし |
| 11 | `request_agent_revocation` | update | 本人セッション | `client_request_id` |
| 11a | `set_network` | update | controllerのみ | なし（設定。冪等） |
| 11b | `set_venue_endpoints` | update | controllerのみ | なし（設定。冪等） |
| 11c | `set_ecdsa_key_id` | update | controllerのみ | なし（設定。冪等） |
| 11d | `get_environment` | query | 公開（診断用。秘密を含まない） | なし |

### 2.1 認証

```candid
type ChallengeRequest = record {
  eoa_address : blob;            // 20バイト
  principal : principal;         // ブラウザ生成の短命IC署名Identity
  purpose : variant { login; withdrawal };
  network : Network;
  origin : text;                 // ブラウザorigin
};
type ChallengeResponse = record {
  challenge_id : Blob32;
  typed_data : blob;             // EIP-712。origin/network/canister/用途/nonce/期限を含む
  nonce : blob;
  expires_at : Timestamp;        // 発行から5分
};

type OpenSessionRequest = record {
  challenge_id : Blob32;
  eoa_signature : blob;
};
```

拒否規則: challenge nonceの再利用、期限切れ、`origin`・`network`・`canister`・`purpose`の不一致、`principal`と署名EOAの不一致。`purpose = withdrawal` のchallengeはセッション確立に使えない。

### 2.2 資金

```candid
type FundingInstructions = record {
  account_kind : AccountKind;
  hl_account_address : blob;     // 本人に提示してよいアドレス
  asset : AssetId;               // 初期は usdc のみ
  network : Network;
  minimum_amount : nat64;        // 未確定（Phase 1で実測）
  memo_required : bool;
};

type AllocationRequest = record {
  client_request_id : RequestId;
  amount : nat64;                // USDC最小単位
  target : variant { trading_account };
  intent_signature : opt blob;   // 必要な場合のみ
};
type WithdrawalRequest = record {
  client_request_id : RequestId;
  amount : nat64;
  asset : AssetId;
  destination : variant { authenticated_eoa_hl_account };
  network : Network;
  nonce : nat64;                 // 資金移動nonce。ログインchallengeと別管理
  expires_at : Timestamp;
  intent_signature : blob;       // 金額・宛先・network・nonce・期限への束縛
};

type FundStatus = record {
  reserve_unallocated : nat64;   // 共通保管の未配分
  in_transit : nat64;            // 移動中（配分・回収）
  reserved_for_withdrawal : nat64;
  trading_equity : nat64;        // HL照合値。未実現PnLを含む場合は内訳を分離
  trading_unrealized_pnl : i64;
  withdrawable : nat64;          // 確定済みの出金可能額
  observed_at : Timestamp;
  revision : nat64;
  unknowns : vec record { action_id : blob; kind : text; since : Timestamp };
};

type FundRequestAccepted = record {
  request_id : RequestId;
  fund_action_id : Blob32;
  state : FundRequestState;      // 受付時点。通常 accepted
  accepted_at : Timestamp;
};
```

拒否規則: 宛先は認証EOAのHL口座に限定する。第三者宛・登録EOA変更・運営によるリセットは実装しない。`amount` がゼロ、精度超過、`network`不一致、`expires_at` 超過、nonce再利用は拒否する。

### 2.3 Agent

```candid
type AgentGeneration = record {
  account_id : Blob32;
  generation : nat64;
  agent_address : blob;
  approved_at : opt Timestamp;
  expires_at : opt Timestamp;
  state : variant { requested; approving; active; expiring; revoked; failed };
};

type AgentStatus = record {
  current : opt AgentGeneration;
  next : opt AgentGeneration;
  revocation_pending : bool;
  observed_at : Timestamp;
};

type AgentRevocationRequest = record {
  client_request_id : RequestId;
  scope : variant { stop_new_orders; revoke_current_and_next };
};
```

- Agent世代の作成は`request_agent_generation`で要求する。`funds_vault`が口座のmaster公開鍵とランダム`account_id`を生成し、本人IDへ束縛する。ユーザーが任意のHL口座アドレスやAgentアドレスを申告する方式にしない（`Implementation.md` 7章）。
- 承認は`funds_vault`が取引口座masterで`approveAgent`に署名して実行し、HLの承認状態を独立照合した後に`active`にする。登録済み`trading_core` caller・口座・世代・導出公開鍵が一致しない依頼は拒否する。
- 失効世代の再利用を禁止する。停止・解除はCanisterが実行し、ユーザー自身のHL直接解除を保証しない。
- builder feeを有効にする場合は別途`approveBuilderFee`を要求し、Agent承認と兼ねない（Phase 3-6）。

- 資金actionの送信と入金の定期照合は、本番はグローバルtimer（`ic-cdk-timers`の`set_timer_interval`・5秒間隔。`init`／`post_upgrade`で再arm）で起動する。heartbeatは使わない（メッセージが無くても毎ラウンド呼ばれ、アイドル時もコストが乗るため）。永続状態（action・仕訳）が正本であり、timerの継続を正しさの前提にしない（失敗は次の起動で再試行し、`trading_core`は手動`sweep`でも再開できる）。

## 3. trading_core

### 3.1 注文

```candid
type OrderKind = variant { market_ioc; limit_gtc };
type Side = variant { buy; sell };

type SubmitOrderArgs = record {
  client_request_id : RequestId;
  account_id : Blob32;
  market : variant { btc_perp; eth_perp };
  side : Side;
  kind : OrderKind;
  quantity : text;               // 正規化十進文字列
  limit_price : opt text;        // market_ioc は必須（スリッページ上限付きIOC指値）
  slippage_tolerance_bps : opt nat32; // market既定 50bps（0.5%）
  reduce_only : bool;
  leverage : opt nat32;          // 既定3、上限5
  trigger : opt record { kind : variant { stop_loss; take_profit }; trigger_price : text; is_market : bool };
  expires_after : opt Timestamp;
};

type SubmitOrderResult = record {
  request_id : RequestId;
  order_id : Blob32;
  cloid : Cloid;
  accepted_state : variant { queued };
  accepted_at : Timestamp;
};

type CancelOrderArgs = record { client_request_id : RequestId; order_id : Blob32; };
type CancelAllArgs = record {
  client_request_id : RequestId;
  market : opt variant { btc_perp; eth_perp };
  include_protective_orders : bool;   // true は確認画面を必須とする
};

type CloseAllOutcome = record {
  submitted : vec SubmitOrderResult;
  failed : vec record { market : text; error : ErrorCode };
};
```

- `trigger`は建玉単位の`positionTpsl`（`reduce_only`必須）としてのみ受け付ける。建玉が存在し、`side`がその建玉の反対売買であることを受付時に検査する。市場価格に対する上下（longのSLは下・TPは上）は、coreがmark価格を持たないため受付では検査せず、取引所の判定に委ねる。
- `close`（部分・全決済）は反対売買のreduce-only注文として送る。`close_position`は比率（bps）指定で1件、`close_all`は建玉ごとに繰り返す（1件の失敗で全体を止めず、銘柄ごとの結果を返す）。決済の数量は建玉数量×比率を`szDecimals`で切り捨てる。価格は画面が公開市況から決めるスリッページ上限を渡す（省略時は観測した建玉からmark価格を近似する）。
- `reduce_only`は新規リスクを増やさないため、リスク予約と取引所データの鮮度ゲートの対象外とする。緊急停止は操作種別を問わず新規受付を止める（停止中は決済も受け付けない）。
- Cancel Allと建玉決済を同じ操作にしない。Cancel Allが保護用SL/TPも消す場合は、UIで明示した`include_protective_orders = true`を要求する。
- MarketはHLのスリッページ上限付きIOC指値として構築する。板に残る注文として扱わない。
- `expires_after`はactionの受付期限であり、板に残る注文の取消期限ではない（`state-machines.md` 4節）。
- 署名方式は2系統ある。取引action（order/cancel/cancelByCloid/updateLeverage）はphantom agent方式（EIP-712 domain `Exchange`／`1`／`1337`／`0x0`）、資金・アカウント操作（`approveAgent`・`usdSend`）はuser-signed EIP-712（domain `HyperliquidSignTransaction`／version `1`／chainIdはactionの`signatureChainId`）。2026-09-19に公式SDKのfixtureと一致を確認した（`docs/phase-1/README.md` 6節）。Agent承認は`funds_vault`がmaster鍵で行い、`trading_core`は行わない（`authority-matrix.md` 4節）。

### 3.2 参照

```candid
type AccountSnapshot = record {
  account_id : Blob32;
  equity : nat64;
  margin_used : nat64;
  withdrawable : nat64;
  unrealized_pnl : i64;
  positions : vec record {
    market : text; size : text; entry_price : text; liquidation_price : opt text;
    unrealized_pnl : i64; leverage : nat32; margin_mode : text;
    stop_loss : opt text; take_profit : opt text;
  };
  open_orders : vec OrderView;
  pending_orders : vec PendingOrderView;   // 受付済み・送信前/送信中のローカル状態
  observed_at : Timestamp;
  revision : nat64;
  data_age_ms : nat64;
};

type OrderView = record {
  order_id : Blob32; cloid : Cloid; market : text; side : Side; kind : OrderKind;
  price : opt text; quantity : text; filled_quantity : text;
  state : OrderState; venue_state : opt text;
  hl_oid : opt nat64; cancel_requested : bool;
  trigger : opt record { kind : variant { stop_loss; take_profit }; trigger_price : text; is_market : bool };
  updated_at : Timestamp;
};

type PendingOrderView = record {
  request_id : RequestId; cloid : opt Cloid; action_state : ActionState;
  since : Timestamp; last_error : opt ErrorCode;
};

// `list_orders` が返す型（`OrderView`の拡張。outboxの送信状態を含む）。
type OrderSummary = record {
  order_id : Blob32; cloid : Cloid; market : text; asset_index : nat32;
  is_buy : bool; kind : text; price : opt text; quantity : text;
  filled_quantity : text; reduce_only : bool; state : OrderState;
  dispatch_state : ActionState; preflight_state : ActionState;
  effective_leverage : nat32; effective_slippage_bps : opt nat32;
  expires_after : opt Timestamp; last_error : opt text;
  cancel_requested : bool; hl_oid : opt nat64;
  trigger : opt record { kind : variant { stop_loss; take_profit }; trigger_price : text; is_market : bool };
  created_at : Timestamp; updated_at : Timestamp;
};
```

| # | メソッド | 種別 | 冪等性キー |
|---|---|---|---|
| 12 | `submit_order` | update | `client_request_id` |
| 13 | `cancel_order` | update（封筒必須） | `client_request_id` |
| 14 | `cancel_all` | update | `client_request_id` |
| 15 | `get_account_snapshot` | update（封筒必須） | なし |
| 16 | `list_orders` | update（封筒必須） | なし（カーソル） |
| 17 | `list_fills` | update（封筒必須） | なし（カーソル） |
| 17a | `close_position` | update | `client_request_id` |
| 17b | `close_all` | update | `client_request_id` |
| 17c | `get_hpke_public_key` | query | なし |
| 17d | `rotate_hpke_key` | update（controllerのみ） | なし |
| 17e | `sweep` | update（controllerのみ） | なし（維持運用。冪等） |
| 17f | `set_venue_endpoints` | update（controllerのみ） | なし（設定。冪等） |
| 17g | `set_ecdsa_key_id` | update（controllerのみ） | なし（設定。冪等） |
| 17h | `get_environment` | query（公開・診断用） | なし |
| 17i | `resolve_unknown_order_preflight` | update（controllerのみ） | `order_id` |

- `close_position`は`(session, client_request_id, market, ratio_bps, limit_price)`を取る。`limit_price`はスリッページ上限で、省略時は観測した建玉から導出する。
- `close_all`は`(session, client_request_id)`を取り、建玉ごとに`client_request_id`から導出した受付IDで反対売買を送る。
- `list_orders`は`Paged<OrderSummary>`（`dispatch_state`・`trigger`を含む）、`list_fills`は
  `Paged<FillView>`を返す。`FillView`は約定時刻・価格・数量・手数料・cloid・`hl_oid`を持つ。
- 受付はHLの受理でも約定でもない。`submit_order`の応答は`queued`のみを返し、HL状態は`get_account_snapshot`または`list_orders`の照合結果で更新する（`Implementation.md` 6.3）。
- `account_id`はvaultから導出した本人の口座と一致しなければ拒否する。新規リスクは口座の最終観測が10秒以内の場合だけ受け付ける。
- `leverage`は省略時3、許容範囲1〜5。注文前に同じ永続nonceで`updateLeverage`を送信し、結果不明なら注文を送らず`unknown`で停止する。
- `slippage_tolerance_bps`はmarket IOCだけで、省略時50、許容範囲1〜10,000。limit GTCで指定した場合は拒否する。`expires_after`は受付時と各POST直前に検査し、署名対象とwire payloadにも含める。
- 封筒必須のメソッドの要求・応答は6節の`HpkeRequest`・`HpkeResponse`で包む。平文はCandidで符号化した上記の引数（`session`とメソッド固有の引数）と応答値である。

### 3.3 送信と照合（sweep）

受付（`submit_order`・`cancel_order`・`cancel_all`・`close_position`・`close_all`）はローカル状態だけを確定し、署名・送信・照合は`sweep`が行う。本番はグローバルtimer（`ic-cdk-timers`の`set_timer_interval`）が5秒間隔で起動し（timerはアップグレードで失われるため`init`／`post_upgrade`で再armする）、停止時の手動実行は`sweep`（controllerのみ）が呼ぶ。heartbeatは使わない（メッセージが無くても毎ラウンド呼ばれ、アイドル時もコストが乗るため）。

- 1回の上限：送信4件・取消4件・照合2口座・注文状態4件/口座（outcallの回数を抑える）。
- 送信（`/exchange`）は**非replicated** POST。HTTP/外側の`ok`だけでなく各statusを解釈し、受理は`open`または即時`filled`、明示拒否は`rejected`＋リスク予約の解放、結果不明は`unknown`とし**再送しない**（リスク予約も解放しない。解消は照合またはcontrollerの確認済み操作で行う）。
- 照合（`/info`）は料金方式v2の**replicated** outcall＋変換関数（`transform_info`）で行う。約定（`userFillsByTime`）は`tid`で冪等に取り込み、建玉（`clearinghouseState`）は**観測の全量**で置き換え、注文状態（`orderStatus`）はoidが分かる未終端注文だけに反映する。replicatedでもHL自体の虚偽や履歴欠落は排除できないため、外部証跡の信頼条件と履歴の完全性は別途検証する。
- 照合の対象は`accounts`に取引所アドレスを保存済みの有効口座で、`account_id`順のカーソルで巡回する（先頭N件固定にしない）。アドレスは本人の署名済み要求の処理中にvaultから一度だけ取得して保存する。
- 自動sweep（timer）は本番ビルドのみで組む。試験ビルドでは明示的な`test_sweep_now`で同じ経路を駆動する（PocketICで試験が待つoutcallと取り違えないため）。

## 4. control_guard

| # | メソッド | 種別 | caller要件 |
|---|---|---|---|
| 18 | `schedule_upgrade` | update | SNS governance principal のみ |
| 19 | `cancel_upgrade(target)` | update | SNS governance principal のみ。対象を指定する |
| 20 | `execute_upgrade` | update | 誰でも可（予約内容が一致する場合のみ） |
| 21 | `get_upgrade_status` | query | 公開（直近の予約を状態を問わず返す） |

```candid
type UpgradeRequest = record {
  target : principal;
  wasm_hash : blob;              // 32バイト
  arg_hash : blob;
};
type UpgradeStatus = record {
  scheduled : opt record {
    request : UpgradeRequest;
    scheduled_at : Timestamp;
    executable_at : Timestamp;   // scheduled_at + 7日
    state : variant { pending; executable; executed; cancelled };
  };
  guard_version : text;
};
```

- `schedule_upgrade`は予約内容の変更を受け付けない。変更は`cancel_upgrade`＋新規予約とし、新しい7日を開始する。
- 任意management call、controller追加・移管、reinstall、削除、単独stop、猶予短縮のAPIを設けない。
- `execute_upgrade`は呼び出し主体を問わないが、予約済み`wasm_hash`・`arg_hash`と一致しないupgradeを拒否する（`threat-test-matrix.md` T-501〜T-505）。

## 5. policy_registry

| # | メソッド | 種別 | caller要件 |
|---|---|---|---|
| 22 | `get_policy` | query | Canister間 callers（資金・注文系） |
| 23 | `get_stop_status` | query | 公開（理由コードのみ） |
| 24 | `set_policy_version` | update | control_guard principal のみ。版は厳密に増加させる |
| 25 | `set_emergency_stop` | update | 限定運営権限のみ。**停止方向のみ**（引数なし） |
| 26 | `clear_emergency_stop` | update | SNS governance principal のみ（記録した解除経路） |
| 27 | `set_operator` / `set_sns_principal` / `set_guard_principal` | update | controller のみ。匿名は拒否 |
| 28 | `get_role_principal` | query | 公開（診断用） |

- 読み取り失敗はfail-closedとし、新規受付・新規リスク増加を停止する。
- 緊急停止の解除・制限緩和は記録したSNS経路（`clear_emergency_stop`）で行う。任意送金・即時upgrade・出金先変更は提供しない。
- 政策の変更は `control_guard` principal に限定し、allowlist の各要素は空・カンマ入り・重複を拒否する。

### 5.1 `Implementation.md` 14.3からの追加

14.3はvault 8・core 4・guard 4の操作を列挙し「初期インターフェースは以下に限定する」としている。本契約では次を設計名として追加する。いずれも権限を拡大しない（読み取り、または7章が既に要求するAgent世代作成、画面が必要とする履歴参照）。

| 追加 | 種別 | 理由 |
|---|---|---|
| `list_fund_events` | query | 資金履歴画面（ロードマップ6章）に必須。カーソル必須 |
| `request_agent_generation` / `get_agent_status` | update / query | `Implementation.md` 7章の承認フローに必須。API名が未定義だった |
| `list_orders` / `list_fills` | query | 注文・約定履歴画面に必須。カーソル必須 |
| `policy_registry` の4メソッド | query / update | `Implementation.md` 3.1がpolicy crateを定義しているがAPIが未定義 |

追加はこの表に留め、これを超える操作を実装する場合は基準文書へ差し戻す。

## 6. HPKE封筒と本人データ経路

`Plan.md` 16.5、`Implementation.md` 6.1・6.3に基づく。

```candid
type HpkeRequest = record {
  key_id : blob;                 // サーバ公開鍵のID
  network : Network;
  canister : principal;
  method : text;
  request_id : Blob32;
  expires_at : Timestamp;
  client_public_key : blob;      // 応答を暗号化するブラウザ公開鍵
  aad : blob;                    // network/canister/method/caller/request_id/期限を束縛
  ciphertext : blob;             // ChaCha20-Poly1305
};

type HpkeResponse = record {
  request_id : Blob32;
  key_id : blob;
  observed_at : Timestamp;
  ciphertext : blob;             // client_public_key宛
};
```

- 方式はRFC 9180 HPKE（X25519 / HKDF-SHA256 / ChaCha20-Poly1305）。監査実績のある実装を使い、プリミティブを自作しない。実装は共有クレート `crates/hpke-envelope`（`funds_vault` と `trading_core` が使う）。
- `key_id`は現行世代の公開鍵そのもの（32バイト）とする。鍵は`rotate_hpke_key`（controllerのみ）で世代を進め、退役した世代の封筒は復号しない（クライアントは公開鍵を取得し直す）。
- 公開鍵・key ID・期限はICの認証済み応答として取得し、クライアントが検証する。
- 認証対象に`network`・`canister`・`method`・`caller`・`request_id`・期限を含め、別環境・別用途への再利用を拒否する。サーバは`aad`を再計算して一致を確認し、同じ値を復号にも使う（改竄は復号失敗になる）。
- HPKEは本人認証・再送防止の代わりではない。セッションと`request_id`の検証を別に行う。`request_id`は復号に成功した要求について単回使用として記録し、再送を拒否する（`BadRequest.NonceReused`）。記録は期限切れで掃除するが、照合に使う観測データ（建玉・約定など）は期限だけでは破棄しない。
- 要求の期限は`now`から300秒以内に限る。期限切れは`BadRequest.ExpiredIntent`で拒否する。
- 適用範囲は`get_account_snapshot`・`list_orders`・`list_fills`・`cancel_order`である。`submit_order`・`cancel_all`・`close_position`・`close_all`・`request_agent_generation`・`get_agent_status`への適用は未実施（Phase 3で判断する）。
- セッション鍵・本人キャッシュをブラウザで永続化しない（`localStorage`・`IndexedDB`・Cookieを使わない）。
- ブラウザのHL直結は公開市況チャネルのみとする。ユーザー系WS/REST、Agent承認、取引口座照合をブラウザからHLへ送らない。

## 7. エラー型

```candid
type ErrorCode = variant {
  Unauthenticated : record { reason : text };
  SessionExpired;
  SessionRevoked;
  NotEligible : record { policy_version : nat64 };
  PolicyUnavailable;
  JournalWriterBusy;
  BadRequest : record { code : BadRequestCode; detail : text };
  IdempotencyConflict : record { request_id : RequestId };
  DuplicateIgnored : record { request_id : RequestId };
  InsufficientFunds : record { available : nat64; requested : nat64 };
  ReservationConflict;
  RiskLimitExceeded : record { limit : nat64 };
  StaleAccountState : record { observed_at : Timestamp; max_age_ms : nat64 };
  UpstreamUnavailable : record { venue : text };
  VenueRateLimited : record { retry_after_ms : opt nat64 };
  UpstreamRejected : record { code : text; retryable : bool };
  UnknownPending : record { action_id : blob };
  SigningQueueFull;
  NotAllowed : record { code : NotAllowedCode };
  Internal : record { code : text };
};

type BadRequestCode = variant {
  MalformedPayload; TooLarge; MissingField; UnsupportedAsset; UnsupportedMarket;
  PrecisionExceeded; QuantityOutOfRange; PriceOutOfRange; InvalidSignature;
  ChallengeReused; ChallengeExpired; NetworkMismatch; OriginMismatch;
  DestinationNotAllowed; NonceReused; ExpiredIntent; AmountZero;
};

type NotAllowedCode = variant {
  OrderNotFound; OrderNotCancellable; AssetNotAllowed; AccountNotOwned;
  CallerMismatch; SessionIssuedByUnregisteredVault;
  UpgradeNotScheduled; UpgradeContentMismatch; UpgradeTooEarly; UpgradeAlreadyExecuted;
  OperationNotAvailable;
};
```

| 分類 | エラー | クライアントの責務 |
|---|---|---|
| 同一`client_request_id`・同一本文で再試行可 | `UpstreamUnavailable`、`VenueRateLimited`、`SigningQueueFull` | 同じ冪等性キーで再送。新しいcloid・nonceを作らない |
| 状態更新後に再試行可 | `StaleAccountState`、`PolicyUnavailable`、`ReservationConflict` | 状態を再取得し、`revision`と`observed_at`を更新してから再判断 |
| 受付ごとの再試行 | `JournalWriterBusy` | 単一書込みフェンスの一時的な競合。`open_session`はchallenge消費後のため、新しいchallengeと署名で有界に再試行する。他の受付は各要求の冪等性規則を守る |
| 再試行不可 | `Unauthenticated`、`SessionExpired`、`SessionRevoked`、`NotEligible`、`BadRequest`、`IdempotencyConflict`、`InsufficientFunds`、`RiskLimitExceeded`、`NotAllowed`、`Internal` | 理由を表示し、入力を修正する。自動再送しない（`Internal`は設定不備・DB不整合を含む恒久エラーのため） |
| 成功扱い（重複受付） | `DuplicateIgnored` | 既存の受付状態を表示する。エラー表示にしない |
| 自動再送禁止（照合のみ） | `UnknownPending` | 結果不明として表示し、照合結果が届くまで再発注しない |
| 上位の拒否 | `UpstreamRejected` | `retryable`に従う。`retryable = false`は入力・リスクを見直す |

- `IdempotencyConflict`は同一`client_request_id`で本文が異なる場合である。同一内容の再送は受理済みの結果を返す（`money-and-units.md` 4節）。
- エラー応答に他ユーザーの残高・注文・口座アドレスを含めない。

## 8. 未確定事項

| 項目 | 確定時期 |
|---|---|
| 実Candid（`.did`）と型の最終形 | Phase 1 |
| レート制限、ページ件数以外の上限値、バッチ上限 | Phase 1（REST weight予算実測と同時） |
| `minimum_amount`、送金手数料、確定イベント取得方法 | Phase 1（testnet実測） |
| Agent世代の承認待ち時間、27日切替の実挙動 | Phase 1（`Implementation.md` 1-4） |
| eligibilityの粒度と`get_policy`の公開範囲 | Phase 3-5 |
| builder feeの上限同意フロー | Phase 3-6（本番の事業判断） |
