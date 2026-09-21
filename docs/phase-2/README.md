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

## 残りの実装（次セッションで着手）

### 1. SL/TP（トリガー注文）
- 型は `hl-types` に既存：`OrderType::Trigger(TriggerOrder { is_market, .., tpsl })`、`Tpsl::{NormalTpsl, PositionTpsl}`、`OrderRequest.reduce_only`。coreの`SubmitOrderArgs.trigger`は受け取れるが**未配線**。
- 手順：(a) `orders`表へ `trigger_price TEXT`・`trigger_is_market INTEGER`・`trigger_tpsl TEXT` を追加（coreスキーマは未リリースなのでv6定義を編集可）。(b) `submit_order` の受付で `args.trigger` を検証（価格>0、SL/TPの向きと建玉の整合、reduce_only必須）。(c) `sign_and_build` の action 構築と JSON本文で `t: {"trigger": {...}}` を出す分岐。(d) 試験：SL付き注文の署名actionに `trigger` が入り、`orderStatus` 反映で状態が変わること。
- 参照：`crates/db/src/schema/core.rs`、`crates/trading-core/src/lib.rs`（`submit_order`・`sign_and_build`）、`crates/hl-types/src/action.rs`。

### 2. 全決済・部分決済（reduce-only）
- 手順：(a) `submit_order` の受付〜action作成〜署名を**内部ヘルパ `submit_inner(user_id, args)` に抽出**（現在は`submit_order`内に直書き）。(b) `close_position(session, market, ratio_bps)` を追加：`positions`の `size` を読み、符号で売買を決め、`reduce_only=true` の IOC 指値を `submit_inner` で送る。(c) `close_all(session)` は建玉ごとに繰り返す。(d) 試験：建玉取り込み→全決済で反対売買のreduce-only注文が出て、`orderStatus`/約定取り込みで建玉が0になること。
- 参照：`crates/db/src/repo/positions.rs`、`crates/trading-core/src/lib.rs`。

### 3. HPKE封筒の個人API適用（T-605）
- 現状：封筒（`seal`/`open`/`envelope_aad`）と鍵レジストリは **funds_vault のみ**（`crates/funds-vault/src/hpke.rs`）。coreには無い。
- 手順：(a) **共有クレート `crates/hpke-envelope` を新設**し、`funds-vault`の封筒実装（`SeededRng`・`seal`・`open`・`envelope_aad`）を移設して`funds-vault`から再輸出（他canisterでも使えるようにする）。(b) coreへ `key_registry`（世代・秘密鍵・公開鍵、`raw_rand`由来）と `get_hpke_public_key` を追加。(c) 契約§6の `HpkeRequest`/`HpkeResponse` に沿って、個人API（`get_account_snapshot`・`list_orders`・`list_fills`・`cancel_order`）を**封筒必須**にし、`request_id`再送拒否と`aad`（network/canister/method/caller/request_id/期限）を検証。(d) 試験：正しい封筒のみ通る・再利用拒否・`aad`改竄拒否・鍵更新中の挙動。
- 参照：`docs/phase-0/api-contract.md` §6、`crates/funds-vault/src/hpke.rs`、`crates/trading-core/src/lib.rs`。

### 実行上の注意（並行作業対策）
- PocketICは必ずスクリプト経由：`POCKET_IC_TEST_DIR=$PWD/target/test-venue-mine bash scripts/pocket-ic-test.sh --test <name>`。
- 素の `cargo test` は本番用wasm（feature無し）を読むため偽の失敗になる。
