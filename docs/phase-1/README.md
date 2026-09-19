# Phase 1（S1–S3）：タスク台帳と証跡

- 作成日：2026-09-19
- 状態：**S1完了（署名fixture）、S2/S3は未着手**。PocketIC基盤は成立確認済み
- 基準：`Implementation-Roadmap.md` 5章・11章、`Implementation.md` 9.1・9.2〜9.4、`docs/phase-0/` の契約

## 1. この台帳の使い方

ロードマップ11章の様式（ID／目的／依存／実装対象／対象外／受け入れ条件／検証／証跡／残件）で記録する。Phase全体を完了扱いせず、証拠のある範囲だけを「完了」とする。

証跡は `docs/phase-1/evidence/` に置き、`docs/phase-0/threat-test-matrix.md` のT-xxxと `Implementation.md` の1-xへ対応付ける。

## 2. 段階と対応

| 段階 | 内容 | 状態 |
|---|---|---|
| S1 | hl-sign（action構築・msgpack・EIP-712・v復元）＋公式SDK比較 | 完了 |
| S2 | PocketIC基盤、ic-sqlite-vfs疎通、複式台帳・予約・outbox・nonce・fencing | 基盤のみ完了、台帳以降は未着手 |
| S3 | モックHL往復・障害注入、ローカルECDSA、guard迂回拒否 | 未着手 |

## 3. 完了したタスク

### P1-001 hl-sign: 非async・純粋なaction構築と署名

- 目的：`Implementation.md` 9.1の5項目を満たす署名実装を作る。
- 依存：なし（Phase 0のworkspace）。
- 実装対象：`crates/hl-types`（decimal・msgpack・action）、`crates/hl-sign`（keccak・eip712・hash・signature・user_signed）。
- 対象外：tECDSA（管理Canister）での実署名、PocketIC、HL接続。
- 受け入れ条件：公式SDKと同一入力でactionハッシュ・署名・復元アドレスが一致する。
- 検証：`cargo test`（`crates/hl-types` 17件、`crates/hl-sign` 23件、`tests/fixtures.rs` 1件）。`bash scripts/check-no-await.sh`。
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

## 4. 未着手のタスク（S2・S3）

| ID | 内容 | 対応 |
|---|---|---|
| P1-004 | `db` のスキーマ・Migration・複式台帳・予約・nonce・epoch CAS | `Implementation.md` 4.3、14.1、1-7 |
| P1-005 | `funds_vault` の認証・台帳・資金outbox・照合・HPKE・API | 14.2、14.3、1-2、1-9 |
| P1-006 | `trading_core` の受付・パイプライン・照合・snapshot | 2.3、5章、1-9 |
| P1-007 | `control_guard` の予約・7日猶予・迂回拒否 | 14.4、1-10 |
| P1-008 | モックHL往復と障害注入（PocketIC） | 9.4、1-5、1-6 |
| P1-009 | ローカルECDSAスパイク（`sign_with_ecdsa` のkey id確定と `v` 復元） | 9.1の4、1-2、1-3（ローカル部分） |
| P1-010 | ローカルで実行可能なT-xxx試験（T-101〜T-108、T-201〜T-207、T-210〜T-212、T-301〜T-307、T-401〜T-408、T-501〜T-506、T-702） | `docs/phase-0/threat-test-matrix.md` |

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
