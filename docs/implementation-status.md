# 実装状況：UI・サーバー基盤とADR

更新：2026-09-22。対象はローカル開発用の合成デモ、Phase 0の契約、Phase 1のS1〜S3（ローカルで検証できた範囲）。**testnet・mainnetは未検証**で、全Phase完了ではない。

## Phase 1（S1・PocketIC基盤）

2026-09-19に、Phase 1のうちローカルで完結する署名部分（S1）と統合試験基盤を実装・検証した。当時は資金台帳・モックHL往復・guardが未着手だったが、**その後S2・S3のローカル範囲まで実装・検証済み**である。最新の状態は `docs/phase-1/README.md` の台帳と本ファイル末尾の節を正とする。

### 完了したもの

- `hl-types`：正規化十進（`decimal`）、canonical msgpack（`msgpack`）、action構築（`action`、order/cancel/cancelByCloid/updateLeverage）。
- `hl-sign`：Keccak-256、EIP-712（phantom agent、`Exchange`/`1`/`1337`/`0x0`）、actionハッシュ（`msgpack ‖ nonce(8B BE) ‖ vault ‖ expires`）、決定的署名と復元、`v` の候補復元、user-signed EIP-712（`HyperliquidSignTransaction`、`ApproveAgent`/`UsdSend`）。
- `api-types`：`docs/phase-0/api-contract.md` のCandid型（CanisterとPocketIC試験で共有）。
- `tools/hl-fixture-gen/`（公式SDK `@nktkas/hyperliquid@0.33.3` を固定）と、`crates/hl-sign/tests/fixtures/` の11件。
- PocketIC基盤：`scripts/fetch-pocket-ic.sh`、`scripts/pocket-ic-test.sh`、`crates/pocket-ic-tests`。

### 検証結果（ローカル実行）

- `cargo test`：ホスト66件成功（PocketIC統合試験は別ジョブ）（`api-types` 5、`db` 9、`hl-sign` 35〔`lib` 33・`fixtures` 2〕、`hl-types` 17）。`hl-sign` には公式SDK fixtureとの比較試験（11件）を含む。`api-types`の1件は`4c0e5e5`で追加した回帰試験で、`46b061d`の時点は65件。
- `bash scripts/pocket-ic-test.sh`：**21ファイル・60試験すべて成功**（`crates/pocket-ic-tests/tests/`）。試験バイナリは21の統合試験ファイルにlib単体試験とDoc-testsを加えた23。
- 公式SDKとの一致：actionハッシュ（`createL1ActionHash`）・署名（r/s/v、決定的）・復元アドレスのすべてが11件で一致。中間値（msgpack・digest）はSDKが公開しないためfixtureには入れず、署名一致で検証している。
- `cargo clippy --workspace --all-targets -- -D warnings`（ホスト全体。`pocket-ic-tests` とCanisterクレートを含む）、`cargo fmt --all --check`、`bash scripts/check-no-await.sh`、`bash scripts/check-signing-boundary.sh`：成功。
- `bash scripts/fetch-pocket-ic.sh`：サーバの版（`pocket-ic-server 16.x`）とsha256を検証する。旧版・digest不一致では非0終了することを確認済み。
- `cargo build --release --target wasm32-unknown-unknown`（4 Canister）：成功。
- `bash scripts/pocket-ic-test.sh`：PocketIC 16.0.0（arm64-darwin）で4 Canisterをdeployし、`version` queryが応答（`init` のDB初期化がtrapしないことを含む）。

### Phase 1で残るもの（当時の記載。ローカル範囲は解消済み）

- 当時の残件だった `db` のスキーマ・Migration・複式台帳・予約・nonce・epoch CAS、`funds_vault` の認証・outbox・HPKE・API、`trading_core` の注文パイプラインとモックHL照合、`control_guard` の7日猶予は、いずれも**実装・ローカル検証済み**。
- ローカルECDSAスパイクは完了（`sign_with_ecdsa` のkey idは `test_key_1`）。PocketICの障害注入とT-xxx試験は一部実行済み（`docs/phase-0/threat-test-matrix.md`）。
- **残る残件**：実HL（testnet）での注文受理・照合の実測、`control_guard` の一致実行のサイズ制約（実サイズwasmは単一ingressで送れない）、testnet往復と実測（別途の環境・承認が必要）、恒久エラー時の運用手順。timer失敗の握り潰しは2026-09-22に解消し、Canisterログへ記録する。T-605（封筒の個人API適用）は2026-09-21に実装・ローカル検証済み（`docs/phase-2/README.md`）。

## Phase 0（契約・画面仕様・Rust雛形）

2026-09-19に、ロードマップ4章の実装契約と画面仕様を `docs/phase-0/` に固定し、12章-2のRust workspace雛形とローカル試験環境を用意した。

### 用意したもの

- 契約文書8本（`docs/phase-0/`）：権限表、API契約とエラー型、状態遷移、金額と一意性、環境分離、脅威と試験の対応、プライバシー評価の入力、画面仕様。
- Rust workspace：`Cargo.toml`（依存を `=` で完全固定、`Cargo.lock` をコミット）、`rust-toolchain.toml`、`icp.yaml`、`crates/`（`hl-types`、`hl-sign`、`db`、`policy`、`funds-vault`、`control-guard`、`trading-core`）、`scripts/check-no-await.sh`。
- （Phase 0の時点では）Canisterは `version` と `db::init`（`ic-sqlite-vfs 2.0.0`、Migrationは空）のみで、資金・署名・注文・照合は未実装だった。**現在は本ファイル末尾のとおり実装・検証済み**。
- CI：`.github/workflows/rust.yml`（fmt / clippy（ホスト・wasm）/ test / no-await検査 / wasmビルド）。**リモートCIは未実行**。
- （Phase 0の時点で）ホストで実行するテスト12件（`hl-types` 4、`hl-sign` 3、`db` 5）。現在は66件。

### 検証結果（ローカル実行）

- ツールチェーン：`rustc`/`cargo` 1.97.0（`rust-toolchain.toml` で固定）。
- `cargo fmt --all --check`：成功。
- `cargo clippy --all-targets -- -D warnings`（ホスト既定メンバー）：成功。
- （Phase 0の時点で）`cargo test`：12件成功。現在は66件。
- `bash scripts/check-no-await.sh`：ok（`hl-sign`・`db` に非async規則違反なし）。
- `cargo clippy --target wasm32-unknown-unknown -p policy -p funds-vault -p control-guard -p trading-core -- -D warnings`：成功。
- `cargo build --release --target wasm32-unknown-unknown`（4 Canister）：成功。各約1.25 MB（`ic-sqlite-vfs` 2.0.0をリンク）。
- `icp project show`：`icp.yaml` のrecipe展開を確認。
- `icp build`：4 Canister成功。`candid-extractor` と `ic-wasm` で `candid:service` を埋め込み、抽出した`.did`とfrontend bindingをリポジトリに同期する。
- `icp network start -d` → `icp deploy` → `icp canister call <name> version --query`：4 Canisterが `("0.1.0")` を返し、`init` のDB初期化がtrapしないことを確認。`icp network stop` で停止し、停止も確認。

### この環境での実行上の注意

- このセッションのサンドボックスはHOME配下へ書き込めないため、`CARGO_HOME=<repo>/.cargo-home` と `ICP_HOME=<repo>/.icp-home` を指定して実行した（両方とも `.gitignore` 済み）。通常の開発環境では不要。
- 当初 `channel = "1.93.0"` を指定したが、`ic-sqlite-vfs 2.0.0` のMSRVが1.95.0であり、かつtoolchainの追加インストールがサンドボックス制約で失敗したため、要件を満たす導入済みの1.97.0を固定した。リリース用の完全固定はPhase 4で行う。
- ローカルのCanister IDはicpが払い出した開発用の値である（`.icp/` 配下、未コミット）。testnet・mainnetの値は未確定。

### Phase 0で残るもの

- 実Candid（`.did`）、レート制限と上限値、HL固有の価格精度・手数料・確定イベント、Agent世代の実挙動。いずれもPhase 1の実測で確定する。
- 実テーブルとMigration（Phase 2-1）。PocketICの失敗試験基盤（Rust版 `pocket-ic` のApple Silicon対応は未確認）。
- （Phase 0の時点で）脅威・試験表（`docs/phase-0/threat-test-matrix.md`）の試験は1件も実行していなかった。現在はローカルで実行可能な分を実行済み（同表の「実行済み」を参照）。

## 完了したもの

- ADR 0001〜0006。設計採用と実証状態を分離した。
- Plan v0.9、Implementation v0.5、ロードマップv1.1への整合。
- 新規TanStack Start＋React＋TypeScript/Vite、Workers＋Static Assetsのローカル配信。
- pnpm固定依存・lockfile、Workers生成型、Oxlint型対応、Oxfmt、tsc、Vitest、Playwright。
- `/`、`/trade`、`/funds`、`/history`。公開ページSSR、口座画面はクライアント描画。
- Lightweight Charts、合成板、注文フォーム、TanStack Table注文一覧、資金確認・履歴。
- 正常受理・部分約定・拒否・unknown・取消競合・古い状態のシミュレーション。
- 合成資金の整数計算、要求IDの冪等性、ログアウト時のメモリ破棄。
- WorkersのGET/HEAD限定、実モード503、CF国コードによる入口制限例、セキュリティヘッダー。
- GitHub Actionsの検証workflow追加。リモートでの実行は未実施。

## 検証結果

- `pnpm build`：成功。Cloudflare Workers向けのビルド。
- `pnpm typecheck`：成功。
- `pnpm lint`：成功。型対応あり。Table v8についてReact Compiler非採用を理由とする1行限定の除外がある。
- `pnpm format:check`：成功。
- `pnpm test`：39件成功。HPKEのclient/server往復・AAD不一致拒否を含む。
- `pnpm test:e2e`：7件成功。build後のWorkers previewをPlaywright Chromiumで検証。
- Playwright CLIによる画面表示・操作・コンソール確認。スクリーンショットは引渡し成果物に保存。

ブラウザ試験でJS/CSS配信の404とチャートautoSizeのレイアウト変動を検出・修正した。ビルド成功だけをUI完成とは扱っていない。Safari・Firefox、実MetaMask、実ICP、性能負荷、実資金の試験は未実施。

## 未実装・次工程を止めている条件

Canisterコードは `version` とDB初期化だけの雛形ではない（資金・署名・認証・注文の業務ロジックを実装済み。本ファイル末尾を参照）。Candidは`candid/`に固定しfrontend bindingも生成済みだが、testnet Canister IDはない。そのため実環境のICP接続段階を保留している。

必要な次の成果物：

1. ~~Rust/PocketICでの資金・署名・認証の実装と検証~~ → ローカル範囲は完了。testnet検証が残る。
2. ~~Candid生成~~とCanister ID、本人認証・失効の実契約。network・endpoint・tECDSA key IDは起動時の設定（controller専用setter）として実装済みで、testnetデプロイ後に実値へ確定する。
3. ~~認証済みHPKE公開鍵の取得・鍵更新・要求と応答の暗号化仕様~~ → 鍵レジストリ・封筒・個人API4件（`get_account_snapshot`・`list_orders`・`list_fills`・`cancel_order`）への適用まで実装・検証済み。`submit_order`等の書き込み系への適用はPhase 3で判断する。
4. ~~注文・資金移動の照合fixtureと、unknownの回復契約~~ → outboxの照合と`unknown`解消を実装・検証済み。`orderStatus`照合の自動化が残る。
5. HL公開市況の接続、口座状態・建玉・PnL・SL/TP・決済の接続。

画面に建玉プレビューはあるが、SL/TP・決済は無効表示。デモで約定したことを根拠に本物の建玉を作らない。単体テストの整数残高モデルではなく、Canister側に複式台帳と永続outboxを実装済みである。

## 本番前の残件

Canisterの資金安全性・機密性・相関耐性、SNS/guard、controller移管、法務、eligibility発行、配信権限分離、nonce対応を含むCSP、依存ライセンス・NOTICEの再確認、独立監査が必要。Cloudflare配信権限が持つJS変更リスクは残る。

Cloudflare公開、SNSローンチ、controller変更、ウォレット接続、実資金操作は行っていない。

## Phase 1（S2・S3）のCanister実装状況（2026-09-21・ローカル検証）

Canister側は「`version`とDB初期化だけの雛形」ではなくなった。資金層（S2）と統制（S3）のローカルで
検証できる範囲が動作し、PocketICで**29ファイル・83試験すべて成功**している（2026-09-21のPhase 2
2C/2D/2E＋T-605＋本番パイプライン＋環境設定の一般化の追加後。Phase 1時点は21ファイル・60試験）。ただし**Phase 1の
Go/No-Goは未合格**であり、testnet往復は未実施。

### 検証済み（証跡: `docs/phase-1/evidence/P1-001`〜`P1-010`）

| 領域 | 状態 | 証跡 |
|---|---|---|
| EOA認証（challenge・セッション・失効・origin束縛・principal束縛T-102） | 実装・検証済み | P1-001〜003 |
| 資金の参照・受付・予約・複式台帳 | 実装・検証済み | P1-004 |
| 入金計上（`/info`搬送路・未知宛先のsuspense計上・controllerによる本人への振替） | 実装・検証済み | P1-004 |
| outbox（claim→実tECDSA署名→`dispatching`永続化→非replicated POST→照合） | 配分・払出し・回収で実装・検証済み | P1-006 |
| 不明な送金の扱い（再送しない・時間経過でも解放しない・`unknown`／`dispatching`の解消） | 検証済み（T-205・T-206） | P1-006 |
| upgradeでの認証・台帳・未解決actionの保存 | 検証済み | P1-010 |
| HPKE（鍵世代の更新・公開鍵配布・封筒の往復・`aad`束縛） | 実装・検証済み | P1-007 |
| `control_guard`（SNS限定・7日猶予・内容一致・迂回APIなし・同時実行の単一性） | 実装・検証済み（実サイズwasmの実行は下記制約で保留） | P1-009 |
| `policy_registry`（fail-closed・停止方向のみ） | 実装・検証済み | P1-010 |
| `trading_core`（認可境界・注文受付・Agent鍵署名・送信・取消送信・約定取り込み・`orderStatus`照合・snapshot・リスク予約・SL/TP・全決済・個人API封筒・環境設定） | 実装・検証済み | P1-008 |
| 環境分離（network・endpoint・tECDSA key IDの起動時設定、mainnet拒否＝E-2） | 実装・検証済み（E-1はeligibility未実装のため未実施） | `core_environment.rs`・`vault_environment.rs` |

ローカルの閾値ECDSAはPocketICの**テスト用閾値鍵サブネット**で有効（key id `test_key_1`）。
PocketIC上の署名往復は約17.9ms（本番subnetの性能値ではない）。

### 未解消・未検証（次段階）

1. `trading_core`：受付（認可・冪等性・allowlist・meta添字）→ Agent鍵での署名 → `dispatching`永続化 → 非replicated送信 → 受理（`open`＋`oid`）／拒否／不明の分類、`get_account_snapshot`、約定の取り込み（`tid`で冪等）、`orderStatus`照合の反映、リスク予約、緊急停止中の受付拒否、入力検証、取消の送信まで検証済み。SL/TP（`positionTpsl`・reduce-only）と全決済・部分決済（`close_position`・`close_all`）、個人APIの封筒必須化・鍵更新まで検証済み。受付後の**送信・取消・`/info`照合（userFills・clearinghouseState・orderStatus）・口座巡回**を本番wasmの`pipeline`／`venue`へ移し、グローバルtimer（本番のみ・5秒間隔）と`sweep`（controller手動）で駆動するようにした（2026-09-21）。**残るのは実HL（testnet）での受理挙動と、timer周期コストの実測。**
2. 2C：入金の受信側（搬送路を含む）、払出しの送信（`payout_settled`／拒否で解放＋逆仕訳／不明で保持）、`unknown`の照合解消、回収（recovery）の送信経路（取引口座のequityに対する予約と、受理・拒否・不明の分岐。`vault_recovery.rs`の3試験で確認）まで検証済み。**残るのは60秒timeoutの再現とtestnetでの実HL受理。**
3. ~~個人データAPIへの封筒適用と応答暗号化、鍵更新中の扱い（T-605）~~ → 2026-09-21に解消（4メソッドの封筒必須化・`request_id`再送拒否・`aad`束縛・鍵更新で旧封筒を拒否。試験 `core_hpke.rs`）。残余：封印するのは`get_account_snapshot`・`list_orders`・`list_fills`・`cancel_order`のみで、`submit_order`・`cancel_all`・`close_position`・`close_all`・`request_agent_generation`・`get_agent_status`は平文（セッション認可のみ）。
4. `control_guard`の一致する実行：実行経路と同時実行の単一性は極小wasmで検証済み。ただし実サイズのwasmは `execute_upgrade` の引数として2 MiB上限を超えるため（実測2,193,336バイト）、チャンク導入かコードレジストリが必要（**未解消**）。
5. 残りの失敗試験（T-401〜T-410等）と、Phase 1完了後の読み取り専用レビュー。
6. 運用面の残件：**恒久エラー（ダイジェスト不一致等）発生時の運用手順**が未定義。定期sweepの失敗はCanisterログへ記録するよう解消済みだが、監視・通知経路は未実装。
7. testnet：実HLの受理挙動、署名p50/p95、受付→HL受理、Confidential Subnetの成立性。**未検証**。
