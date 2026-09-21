# Phase 2：単一ユーザーtestnet MVP（計画と進行）

`Implementation-Roadmap.md` §6 の実装計画。Phase 1の細部の作り込みより、**testnetで資金往復を通すこと**を優先する。

## GATE 0：前提（未達なら testnet へ進まない）

| # | 前提 | 状態 | 必要な操作 |
|---|---|---|---|
| G0-1 | IC testnet用identityとcycles、4 canisterのデプロイ承認 | **未** | `icp identity`でidentity作成 → faucetでcycles取得 → `icp deploy`（`funds_vault`/`trading_core`/`control_guard`/`policy_registry`） |
| G0-2 | HL testnet口座＋test USDC、MetaMask | **未** | HL testnet faucetでUSDC受領、MetaMaskにtestnetを追加 |
| G0-3 | testnet tECDSA key IDの確定（`test_key_1`第一候補・未確認） | **未** | デプロイ後 `ecdsa_public_key` を実測し `docs/phase-0/environments.md` へ記録 |
| G0-4 | HL testnetの制限（最小額・手数料・確定イベント） | **未** | 預入・出金を1往復して実測 |

## マイルストーン

| M | 内容 | ゲート |
|---|---|---|
| M1 | GATE 0 | 4 canisterがtestnetで起動し、key id確定 |
| M2 | 2A 環境設定の一般化／2B HPKE封筒の個人API適用 | PocketIC＋E-1/E-2 |
| M3 | 2C 建玉・PnL・SL/TP照合／2D 取消・Cancel All・決済／2E 鮮度ゲート | PocketIC、ローカルで代替フロー |
| M4 | 3A ウォレット認証／3B IC接続＋封筒クライアント／資金フロー | testnetで預入→配分→回収→出金 |
| M5 | 3C 取引画面／3D 非正常状態 | 受け入れシナリオ3〜5 |
| M6 | 3E 最小代替クライアント／Playwright／計測レポート／終了レビュー | §6の完了条件 |

## Phase 2で扱わないもの
Phase 3以降（複数ユーザー分離・負荷・backup復元・cycles通知・eligibility/監査/保持削除）、実資金・mainnet（E-2で**拒否**を試験）、Phase 1の細部（UIの磨き込み等）。ただし `reconcile_all` の固定窓（入金先が3件以上で古い口座が対象外）は実バグのためM3までに修正する。

## 残りの実装（2026-09-21に1〜3を実装・ローカル検証済み）

### 1. SL/TP（トリガー注文）— **完了**
- `orders`表に`trigger_is_market`を追加し、`trigger_kind`・`trigger_price`と3列揃いのCHECKを付けた（`trigger_kind`が`tpsl`の役割を兼ねる）。`NewOrder`・`SignableOrder`・`OrderSummary`・`OrderView`まで配線した。
- 受付検証（`submit_inner`）：`reduce_only`必須、トリガ価格の正値と精度（価格と同じ条件）、建玉の存在と`side`が建玉の反対売買であること。市場価格に対する上下は、coreがmark価格を持たないため受付では検査しない（取引所が最終判定）。
- 署名action：`OrderType::Trigger`＋`Grouping::PositionTpsl`を`order_action`（msgpack用）と`order_action_json`（送信用JSON）の共通経路で出す。受付fingerprintにもトリガを含める。
- 試験：`crates/pocket-ic-tests/tests/core_triggers.rs`（受付検証・署名actionの一致・送信本文・`orderStatus`反映）。

### 2. 全決済・部分決済（reduce-only）— **完了**
- `submit_order`の受付〜登録を`submit_inner(user_id, args)`へ抽出した。
- `close_position(session, client_request_id, market, ratio_bps, limit_price)`と`close_all(session, client_request_id)`を追加（`limit_price`はスリッページ上限。省略時は観測した建玉からmark価格を近似）。数量は建玉数量×比率を`szDecimals`で切り捨てる。
- `reduce_only`はリスク予約と鮮度ゲートの対象外にした（建玉があるときに決済・保護を打てなくなる方が危険）。緊急停止は従来どおり決済も止める。
- 建玉の取り込みを全量置換にした（決済済みの建玉が残らない）。`close_all`の受付IDは`client_request_id`と銘柄から導出する。
- 試験：`crates/pocket-ic-tests/tests/core_close.rs`（反対売買・部分比率・全決済・送信と約定で建玉0）。

### 3. HPKE封筒の個人API適用（T-605）— **完了**
- 封筒実装を共有クレート `crates/hpke-envelope` へ移設し、`funds-vault`は再輸出、`trading-core`は同クレートを使う。
- coreへ`hpke_keys`（v7）・`hpke_requests`（v8）を追加し、`rotate_hpke_key`（controllerのみ）と`get_hpke_public_key`を実装。
- `get_account_snapshot`・`list_orders`・`list_fills`・`cancel_order`を**封筒必須**にした。`key_id`（現行公開鍵）・`network`・`canister`・`method`・`caller`・`request_id`・期限を束縛し、`aad`は再計算して一致を確認（改竄は復号失敗）。`request_id`は復号成功時に単回使用として記録し再送を拒否する。応答は`client_public_key`宛に封をする。
- 未適用：`submit_order`・`cancel_all`・`close_position`・`close_all`・`request_agent_generation`・`get_agent_status`（契約§6の適用範囲を4メソッドとしたため）。
- 試験：`crates/pocket-ic-tests/tests/core_hpke.rs`（正規の往復・期限・改竄・caller/canister/network/method束縛・request_id再利用拒否・鍵更新）。

### 4. 本番の注文パイプラインと照合（M3残余）— **完了**
- `test-venue`限定だった署名・送信・照合を `crates/trading-core/src/venue.rs`（outcall・変換）と `pipeline.rs`（送信・照合）へ移し、**feature無しの本番wasmで動作**するようにした。`submit_order` は従来どおり受付（`pending`）だけを確定する。
- 送信（`/exchange`）は非replicated POST。受理は`open`＋oid、拒否は`rejected`＋リスク予約の解放、結果不明は`unknown`（**再送しない**・予約も解放しない）。取消も同じ経路で送る。
- 照合（`/info`）はreplicated outcall＋決定論的な変換関数`transform_info`（用途別に必要な要素だけを残す）。`userFills`は`tid`で冪等に取り込み、`clearinghouseState`は観測の全量で置き換え、`orderStatus`はoidが分かる未終端注文だけに反映する。
- 起動はグローバルtimer（`ic-cdk-timers`の`set_timer_interval`・本番ビルドのみ・5秒間隔）で、`init`／`post_upgrade`で再armする。`sweep`（controller手動）が同じ`sweep_once`を呼ぶ。heartbeatは使わない（毎ラウンド呼ばれ、アイドル時もコストが乗るため）。試験ビルドでは自動sweepを組まず、`test_sweep_now`で決定的に駆動する（PocketICで試験が待つoutcallと取り違えないため）。
- 照合対象は`accounts`に取引所アドレスを保存済みの有効口座で、`account_id`順のカーソルで巡回する（固定窓にしない）。アドレスは本人の署名済み要求の処理中にvaultから一度だけ取得して保存する（`cache_trading_address`）。
- 上限：送信4件・取消4件・照合2口座・注文状態4件/口座。結果は`SweepOutcome{dispatched, cancels, reconciled}`で返す。
- リスク予約の解放：注文が終端（約定・取消・拒否）になった時点で予約を解放する（`state-machines.md` 5節）。解放しないと予約が永久に残り、equityに対する新規注文の枠を食い潰す。建玉の証拠金は取引所の観測（`clearinghouseState`）が表すため、予約は未約定注文のリスクを表す。
- 試験：`crates/pocket-ic-tests/tests/core_pipeline.rs`（送信→照合の往復、拒否・不明の分類と非再送、約定後の予約解放、3口座の巡回、controller限定の手動sweep）。既存の送信試験は`venue_router_default`（送信応答のみ指定し、照合は既定応答）で駆動する。
- 未実施：実HLへの送信はGATE 0後。自動timerは本番ビルドにしか組まないため、間隔・再arm・周期コストはtestnetデプロイ時に実測する（timerが失われても永続状態と手動`sweep`で再開できる）。`state-machines.md` 5節が求めるcycles予算の上限（残cyclesが閾値未満ならsweepを休止する等）は件数上限のみで未実装。建玉の証拠金は取引所の`marginSummary.totalMarginUsed`を取り込んでおらず、`margin_used`は未約定注文の予約合計を表す。

### 実行上の注意（並行作業対策）
- PocketICは必ずスクリプト経由：`POCKET_IC_TEST_DIR=$PWD/target/test-venue-mine bash scripts/pocket-ic-test.sh --test <name>`。
- 素の `cargo test` は本番用wasm（feature無し）を読むため偽の失敗になる。
- 封筒を使う試験は、controllerが `rotate_hpke_key` を呼んで鍵を生成しておく（未生成の個人APIはfail-closedで拒否する）。
