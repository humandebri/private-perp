# Phase 1（S1–S3）：タスク台帳と証跡

- 作成日：2026-09-19（最終更新：2026-09-21）
- 状態：**S1〜S3のローカル検証済み範囲まで完了**（証跡P1-001〜P1-010）。**Phase 1のGo/No-Goは未合格**で、testnet往復は未実施
- 基準：`Implementation-Roadmap.md` 5章・11章、`Implementation.md` 9.1・9.2〜9.4、`docs/phase-0/` の契約

## 1. この台帳の使い方

ロードマップ11章の様式（ID／目的／依存／実装対象／対象外／受け入れ条件／検証／証跡／残件）で記録する。Phase全体を完了扱いせず、証拠のある範囲だけを「完了」とする。

証跡は `docs/phase-1/evidence/` に置き、`docs/phase-0/threat-test-matrix.md` のT-xxxと `Implementation.md` の1-xへ対応付ける。

## 2. 段階と対応

| 段階 | 内容 | 状態 |
|---|---|---|
| S1 | hl-sign（action構築・msgpack・EIP-712・v復元）＋公式SDK比較 | 完了（P1-001・P1-002） |
| S2 | PocketIC基盤、ic-sqlite-vfs疎通、複式台帳・予約・outbox・nonce・fencing | 2A〜2C完了・2D一部完了（P1-003〜P1-007）。2E（PocketIC失敗試験）は一部実行 |
| S3 | モックHL往復・障害注入、ローカルECDSA、guard迂回拒否 | ローカル範囲は完了（P1-005・P1-008〜P1-010）。`trading_core`の注文受付・Agent鍵署名・送信・取消送信・照合・snapshot・リスク予約、`control_guard`の7日猶予・実行・内容不一致拒否、`policy_registry`のfail-closedと停止操作を検証済み。testnet往復は未実施 |

## 2.1 S2の進捗（2026-09-21）

| 段階 | 状態 | 内容 |
|---|---|---|
| 2A | 完了 | `crates/db` に vault/core のスキーマ（バージョン付きMigration）、複式台帳（仕訳合計0・符号付きpostings・残高導出）、資金要求の冪等な受付、予約、challenge/セッション、epoch CASを実装。PocketICで `Db::init`→`migrate` がCanister install時に通ることを確認。 |
| 2B | 完了 | EOA challenge・セッション・失効（`auth.rs`）、資金の参照APIと配分・出金の受付＋予約（`fund.rs`）を実装・検証（`P1-004`）。challengeの`principal`束縛と出金intentのnonce単回使用を含む。 |
| 2C | 完了 | ECDSAスパイク完了（`P1-005`、ローカルで実tECDSA動作）。outboxは配分・払出し・回収の3種を実装し、claim→署名→`dispatching`永続化→非replicated POST→照合まで検証（`P1-006`）。`unknown`の再送禁止・時間経過でも解放しない・`unknown`／`dispatching`の解消、入金の搬送路（`/info`のreplicated outcall）と未知宛先のsuspense計上、`unknown`解消も検証済み。Agentは世代要求・状態表示に加え、master鍵での`approveAgent`署名と受理時の`active`遷移まで実装・検証済み。upgradeでの認証・台帳・未解決actionの保存と非再送も検証済み。 |
| 2D | 一部完了 | 鍵レジストリ（世代更新・公開鍵配布）と封筒の暗号化・復号を検証（`P1-007`）。`aad`が呼び出し元・期限を束縛。個人データAPIへの適用と鍵更新中の扱い（**T-605**）は未着手。 |
| 2E | 一部完了 | PocketIC失敗試験（T-1xx／T-2xxのローカル分）を実行済み。残りはT-401〜T-410等。 |
| — | 完了 | outbox/events repo（`fund_actions`・`external_events` の操作）。 |

出金intentの型は `PrivatePerpWithdrawal(address eoa,uint64 amount,string asset,string destination,string network,uint64 nonce,uint64 expiresAt,bytes canister)` とする。クライアントが知り得ない内部ID（user_id・account_id）を署名対象に含めず、認証済みEOAへ束縛するため（`hl-sign::private_perp`）。

outboxの証跡: `crates/pocket-ic-tests/tests/vault_outbox.rs`（**9件成功**。配分の移動中への計上、応答喪失時の再送禁止、取引所拒否時の予約解放、払出しの送信と`payout_settled`、`unknown`の未実行解消、同時sweepの単一実行、改竄ダイジェストの非署名）。

資金APIの証跡: `crates/pocket-ic-tests/tests/vault_funds.rs`（**6件成功**。残高不足の拒否、冪等な再送と本文相違の拒否、予約による出金可能額の減少、別鍵・宛先相違・期限切れintentの拒否、別callerのセッション拒否、出金intentのnonce再利用拒否）。

認証の証跡: `crates/pocket-ic-tests/tests/vault_auth.rs`（**7件成功**。正しい署名でセッション発行、別鍵・challenge再使用・期限切れ・別originの拒否＝T-101/T-103/T-104/T-105、challengeのprincipal束縛＝T-102、申告principalと呼び出し元の不一致拒否）。

入金の証跡: `crates/pocket-ic-tests/tests/vault_deposits.rs`（2件成功）、`vault_reconcile.rs`（1件成功）、払出し元の回収は `vault_recovery.rs`（3件成功）。

補足: 自前のEIP-712スキーム（challenge・出金intent）は `hl-sign::private_perp` に実装し、フィールド束縛と署名復元をホストテストで固定した。`db::init` はCanisterごとのMigration一覧を受け取る形へ変更した（`policy`・`control_guard` は専用スキーマ未定義のため空で初期化）。

## 3. 完了したタスク

### P1-001 hl-sign: 非async・純粋なaction構築と署名

- 目的：`Implementation.md` 9.1の5項目を満たす署名実装を作る。
- 依存：なし（Phase 0のworkspace）。
- 実装対象：`crates/hl-types`（decimal・msgpack・action）、`crates/hl-sign`（keccak・eip712・hash・signature・user_signed）。
- 対象外：tECDSA（管理Canister）での実署名、PocketIC、HL接続。
- 受け入れ条件：公式SDKと同一入力でactionハッシュ・署名・復元アドレスが一致する。
- 検証：`cargo test`（`crates/hl-types` 17件、`crates/hl-sign` 33件、`tests/fixtures.rs` 2件）。`bash scripts/check-no-await.sh`。
- 証跡：`docs/phase-1/evidence/P1-001.md`。
- 残件：なし（S1の範囲）。

### P1-002 公式SDKのテストベクトル固定

- 目的：`Implementation.md` 9.1の「テストベクトルをリポジトリに固定し、生成元のSDKバージョンを記録する」を満たす。
- 実装対象：`tools/hl-fixture-gen/`（`@nktkas/hyperliquid@0.33.3`・`viem@2.56.8` を固定）、`crates/hl-sign/tests/fixtures/`（11件）。
- 対象外：msgpackバイト列・EIP-712 digest（SDKが公開していないため `null`）。
- 受け入れ条件：fixtureが決定的に再生成でき、Rustの計算と一致する。
- 検証：`cd tools/hl-fixture-gen && pnpm install --frozen-lockfile && pnpm generate` を2回実行して同一出力。`cargo test -p hl-sign --test fixtures`。
- 証跡：`docs/phase-1/evidence/P1-002.md`、`tools/hl-fixture-gen/README.md`。
- 残件：なし。

### P1-003 PocketICハーネスの成立確認（S2の前提）

- 目的：PocketICでCanister統合・障害注入試験を実行できる基盤を作る。
- 実装対象：`scripts/fetch-pocket-ic.sh`、`scripts/pocket-ic-test.sh`、`crates/pocket-ic-tests`、`crates/api-types`。
- 対象外：障害注入の各シナリオ（S3）。
- 受け入れ条件：PocketICサーババイナリを取得し、4 Canisterをdeployして `version` queryが応答する。
- 検証：`bash scripts/pocket-ic-test.sh`（`tests/spike.rs`）。aarch64-apple-darwinでPocketICサーバ16.0.0を使用。
- 証跡：`docs/phase-1/evidence/P1-003.md`。
- 残件：CIでの実行（ubuntu、`pocket-ic-x86_64-linux.gz`）はworkflow追加済み・**未実行**。

## 4. 完了したタスク（P1-004〜P1-010）

かつて「未着手」としていたP1-004〜P1-010は、いずれも証跡と対応する試験が存在する。実際に実施した範囲は当初の対応表と一部異なるため、証跡と実ファイルに合わせて記す。件数は `#[test]` の実測値。

| ID | 実施内容 | 証跡 | 対応する試験（件数） |
|---|---|---|---|
| P1-004 | 資金API（参照・受付・予約・複式台帳）と入金の受信側（`/info`搬送路、未知宛先のsuspense計上と本人への振替） | `docs/phase-1/evidence/P1-004.md` | `vault_funds.rs`（6）、`vault_deposits.rs`（2）、`vault_reconcile.rs`（1） |
| P1-005 | ローカルECDSAスパイク（`sign_with_ecdsa` のkey id `test_key_1` 確定と `v` 復元） | `P1-005.md` | `ecdsa_spike.rs`（1） |
| P1-006 | 資金outboxの署名送信と照合（配分・払出し・回収、`unknown`の保持と解消） | `P1-006.md` | `vault_outbox.rs`（9）、`vault_recovery.rs`（3） |
| P1-007 | HPKE（鍵レジストリ・封筒の往復・`aad`束縛） | `P1-007.md` | `vault_hpke.rs`（1）、`hpke_roundtrip.rs`（1）、`vault_hpke_envelope.rs`（1） |
| P1-008 | `trading_core` の認可境界・注文受付・Agent鍵署名・送信・取消送信・照合・snapshot・リスク予約 | `P1-008.md` | `core_auth.rs`（1）、`core_orders.rs`（11）、`core_order_validation.rs`（1）、`core_risk.rs`（1） |
| P1-009 | `control_guard` の予約・7日猶予・内容一致・同時実行の単一性 | `P1-009.md` | `guard_upgrade.rs`（5） |
| P1-010 | ローカルで実行可能なT-xxx試験とセッション検証、`policy_registry`、upgrade保存、Agent承認 | `P1-010.md` | `vault_auth.rs`（7）、`vault_session_status.rs`（1）、`policy_stop.rs`（2）、`core_policy_stop.rs`（1）、`vault_upgrade.rs`（1）、`vault_agents.rs`（1）、`spike.rs`（1） |

### 4.1 残件（次段階）

- 個人データAPIへの封筒適用と鍵更新中の扱い（**T-605**・未解消）。
- `control_guard` の一致する実行：実行経路と同時実行の単一性は極小wasmで検証済みだが、実サイズのwasmは `execute_upgrade` の引数上限（2 MiB）を超えるため、チャンク導入かコードレジストリが必要（実測2,193,336バイト・**未解消**）。
- 残りの失敗試験（T-401〜T-410等）とPhase 1完了後の読み取り専用レビュー。
- 恒久エラー（ダイジェスト不一致等）発生時の運用手順の定義、定期sweep（timer）の失敗握り潰しの解消（**未解消**）。
- testnet：実HLの受理挙動、署名p50/p95、受付→HL受理、Confidential Subnetの成立性（**未検証**）。
- CI（ubuntu）でのPocketIC実行はworkflow追加済み・**未実行**。

## 5. 実測環境（再現用）

- ツールチェーン：`rustc`/`cargo` 1.97.0（`rust-toolchain.toml`）。
- このマシンではHOME配下へ書き込めないため、実行時に `CARGO_HOME=<repo>/.cargo-home`、`ICP_HOME=<repo>/.icp-home`、`POCKET_IC_BIN=<repo>/.pocket-ic/pocket-ic` を指定した（すべて `.gitignore` 済み）。`scripts/pocket-ic-test.sh` は互換性のない環境変数 `POCKET_IC_BIN` を無視してワークスペースのバイナリを使う（`POCKET_IC_BIN_OVERRIDE=1` で上書き）。
- PocketICサーバ：`release-2026-09-18_03-28-base` の `pocket-ic-arm64-darwin.gz`（`pocket-ic-server 16.0.0`）。
- 公式SDK：`@nktkas/hyperliquid@0.33.3`（fixture生成時のみ。Rust実装は依存しない）。

## 6. 判明した仕様（Phase 1で確定し、契約へ反映するもの）

- actionハッシュの連結は `keccak256( msgpack(action) ‖ nonce(8B BE) ‖ vault ‖ expires )`。vaultは常にマーカー1バイト（`0x00`／`0x01`+20バイト）、expiresは未指定なら0バイト、指定時は `0x00` マーカー＋8バイトBE（公式SDK `esm/signing/_l1.js`）。
- msgpackは整数を最小表現で符号化する。ただし公式SDKは `|値|` がint32範囲外の整数をBigIntへ広げ、正値は `0xcf`（uint64）、負値は `0xd3`（int64）になる。金額・数量・価格は必ず文字列（msgpack str）で渡す。
- 資金・アカウント操作（`approveAgent`・`usdSend`）はphantom agentではなく **user-signed EIP-712**（domain `HyperliquidSignTransaction`／version `1`／chainId = `action.signatureChainId`／verifyingContract `0x0`）。
- `signature_hex` は `r‖s‖v`（65バイト、`v` は27/28）。
- asset indexは `meta.universe` から解決する。testnetは BTC=3・ETH=4、mainnetは BTC=0・ETH=1（`docs/phase-0/money-and-units.md` 3節の方針どおり、indexを固定値として埋め込まない）。

これらは `docs/phase-0/api-contract.md`・`state-machines.md` の契約と矛盾しない。実装上の確定値として `docs/phase-0/environments.md`・`money-and-units.md` に追記する。

## 7. レビュー指摘への対応（2026-09-19）

S1のコミット差分に対する読み取り専用レビューの指摘（P3×8件）への対応。資金・署名の挙動は変えていない。

| 指摘 | 対応 | 検証 |
|---|---|---|
| CIの未使用 `corepack enable` | pocket-icジョブから削除 | `bash -n` とworkflowの目視 |
| `pocket-ic-tests` がlint対象外 | ホストlintを `cargo clippy --workspace --all-targets` へ拡大 | workspace全体でクリーン（この拡大で `manual_is_multiple_of` を1件検出・修正） |
| 取得バイナリの版・完全性検証なし | `POCKET_IC_SERVER_MAJOR`／`POCKET_IC_SHA256` を追加し、キャッシュ利用時と取得後の両方で検証 | 旧版スタブで非0終了、改竄相当（digest不一致）で非0終了、実バイナリで成功 |
| `ActionStateView` の二重定義 | 削除し `fund::ActionState` へ統合 | `cargo test -p api-types` |
| 生鍵署名ヘルパの公開 | `sign_digest_for_tests`／`sign_action_for_tests` へ改名し、`scripts/check-signing-boundary.sh` で canisterクレートからの参照を禁止 | 境界チェックの正常系0件・違反時に非0終了 |
| `preserve_order` の機能統合伝播 | dev-dependencyのfeatureを外し、テスト側の `OrderedJson`（Visitorでドキュメント順を保持）へ置換 | `cargo tree -e features -p hl-sign` に `preserve_order` なし。fixture比較は11件一致のまま |
| テストヘルパのhex長未検証 | 偶数長チェックを追加 | 既存fixture11件で成功 |
| デッドvariant `SigningFailed` | 削除 | `clippy --workspace` クリーン |

追加した回帰テスト：`ordered_json_rejects_floats_and_keeps_key_order`（浮動小数点の拒否とキー順の保持）。

## 8. レビュー指摘への対応（2026-09-21）

資金パスと文書の読み取り専用レビューへの対応。コミットと内容は次のとおり。

| コミット | 対応内容 |
|---|---|
| `8fa3fea` | CIのcargoコマンドへ `--locked` を付け、wasm lintへ `--all-targets` を追加し、ホストでビルドできる全メンバーで試験する。actionsをSHA固定にし、本番ビルドwasmへテスト専用メソッド（`test_*`）が混入していないことを検査するステップを追加。 |
| `97ae049` | 配分の署名者を準備口座のmaster鍵へ揃え（従来は毎回新しい乱数`account_id`から導出した鍵で、保存済み口座と食い違っていた）、`derivation_path`を実際の導出経路にする。出金予約の二重控除で残高参照が`Invariant`エラーになる問題を修正。未知宛先の入金は未帰属勘定へ計上する。定期照合は正の額だけを計上し、1件の失敗で巡回を止めず、`(created_at, master_address)`のカーソルで巡回する。 |
| `46b061d` | challengeを呼び出し元principalへ束縛し（T-102）、束縛確認を消費の前に行う。署名済み出金intentのnonceを単回使用にする。回収は取引口座のequityに対して予約し、受理で消費・拒否で解放する。取引口座への着金は移動中の範囲内だけを配分の確定とし、超過分は取引口座への直接入金として与信する。不明actionの解消は`dispatching`も対象にし、取引所へ照会した証跡を必須にする。`classify_sql`は一意制約のみ`Conflict`とする。 |

各コミットの記録: `8fa3fea` はCIのみ。`97ae049` はコミットメッセージにPocketIC 21ファイル・54試験成功、`46b061d` はホスト65件・PocketIC 57試験成功と記録されている。

`4c0e5e5` 時点での再測（本ラウンド）: **ホスト66件成功**（`api-types` 5、`db` 9、`hl-sign` 35〔`lib` 33・`fixtures` 2〕、`hl-types` 17）、**PocketIC 21ファイル・58試験**。`fdf0512`（policy/guardのfail-closed化）でguardの2試験を追加し**60試験**（`#[test]` と `cargo test -p pocket-ic-tests -- --list` の実測。21の統合試験ファイルにlib単体試験とDoc-testsを加えた試験バイナリは23）。

本ラウンドで解消した項目：回収（recovery）の送信経路、`unknown`解消の対象範囲（`dispatching`を含む）、入金照合の負値による全停止と巡回漏れ、T-102、challengeのprincipal束縛。

未解消として残る項目：`control_guard`の一致実行のサイズ制約（実サイズwasmは2 MiB上限を超える）、T-605、testnet未検証、恒久エラー時の運用手順、定期sweep（timer）の失敗握り潰し。

