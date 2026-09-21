# Phase 0：実装契約・画面仕様

- 作成日：2026-09-19
- 状態：契約を固定。実資金・testnet接続は未着手（Canister実装はPhase 1でローカル範囲まで進行。`docs/phase-1/README.md` と `docs/implementation-status.md` を参照）
- 基準文書：`Plan.md` v0.9（特に3章・16章）、`Implementation.md` v0.5（特に3章・4章・5章・9章・14章）、`Implementation-Roadmap.md` v1.1（特に4章・10章・12章）

## 1. 目的

Phase 0の完了条件は「別の実装者が見ても、成功・失敗・再試行の責務を判断できること」である（ロードマップ4章）。本ディレクトリは、Phase 1以降の実装前に固定する契約と画面仕様を置く。

ここに書かれたAPI名は**設計上の名前**であり、デプロイ済みCandidでも実装済みコードでもない。Canister ID、testnet鍵ID、HLの制限値など未確定の値は「未確定」と明示し、推測値を書かない。

## 2. 成果物索引

| 文書 | 内容 |
|---|---|
| `authority-matrix.md` | Canister責務、許可caller、保持鍵、署名可能action、禁止事項 |
| `api-contract.md` | Canister別API、エラー型と再試行分類、HPKE封筒、上限 |
| `state-machines.md` | action・資金要求・注文の状態遷移、照合規則、UI正準ラベル |
| `money-and-units.md` | 金額の整数単位、丸め、最大値、nonce、重複イベント、保持期間 |
| `environments.md` | local/testnet/mainnet の分離、鍵・endpoint・mock issuer |
| `threat-test-matrix.md` | 脅威と試験の対応、期待結果、証跡、実装Phase |
| `privacy-evaluation.md` | A/B0/B1比較の入力、攻撃者可視情報、合格基準 |
| `ui-spec.md` | 画面構成、非正常状態、残高区分、説明文言、チャート方針 |

追加成果物（ロードマップ12章-2）：Rust workspace雛形と固定依存（リポジトリ直下の `Cargo.toml`、`rust-toolchain.toml`、`icp.yaml`、`crates/`、`scripts/check-no-await.sh`）。

## 3. ロードマップ4章との対応

### 実装前に固定するもの

| ロードマップ項目 | 対応 |
|---|---|
| 各Canisterの責務、許可caller、署名できるactionを表にする | `authority-matrix.md` 2〜4節 |
| 認証・注文・資金・変更予約のAPIとエラー型を定義する | `api-contract.md` 2〜5節 |
| 出金予約、移動中資金、unknown、注文取消の状態遷移を固定する | `state-machines.md` 2〜6節 |
| 金額の整数単位、丸め、最大値、重複イベントの扱いを定義する | `money-and-units.md` 2〜5節 |
| ローカル・testnet・mainnetのID、鍵、endpoint、mock issuerを分離する | `environments.md` 2〜5節 |
| 脅威と試験を対応付ける（二重送金、認可迂回、古いcallback、悪意あるupgradeを含む） | `threat-test-matrix.md` 3節 |
| プライバシー比較の入力、攻撃者に渡す情報、合格基準を固定する | `privacy-evaluation.md` 2〜5節 |

### UI設計

| ロードマップ項目 | 対応 |
|---|---|
| デスクトップの取引画面、資金画面、履歴画面の構成を決める | `ui-spec.md` 2〜4節 |
| 未接続、残高不足、送信中、結果不明、データ遅延、停止中の画面を定義する | `ui-spec.md` 5節 |
| 「保管残高」「取引口座equity」「出金可能額」を別の値として扱う | `ui-spec.md` 6節 |
| 機密性の説明、Canister保管、EOA紛失、停止時の回収制約の文言を作る | `ui-spec.md` 7節 |
| チャートの必要機能と利用条件を確認し、採用候補を絞る | `ui-spec.md` 8節 |

## 4. 完了条件と未達の扱い

- 完了条件：上記の全項目が本ディレクトリの文書で説明され、APIごとに成功・失敗・再試行・照合の責務が読み取れること。
- 本ディレクトリの記述は設計契約であり、実装・実機検証の証跡ではない。Phase 1の実測結果と矛盾した場合は、実測を記録した上で契約を更新する。
- 資金・署名・認証・guardに関わる契約は、正常系の記述だけで完了としない。`threat-test-matrix.md` の失敗試験が未実行である限り、Phase 1のGo/No-Goは未達である。

## 5. 対象外

- hl-signの本格実装（action構築・msgpack・EIP-712・v復元）、DBスキーマの実テーブルとMigration、HL testnet往復、PocketICの失敗試験基盤、Candid `.did` の固定、frontendの変更、チャート採用の確定。
- Rust版PocketIC（`pocket-ic` crate）の採用はPhase 1で判断する。docs.rsのビルド対象は `x86_64-unknown-linux-gnu` のみで、Apple Siliconでの動作は未確認である。
- testnet/mainnetの実値（Canister ID、署名鍵ID、HLの上限・手数料）はPhase 1の実測で確定する。

## 6. 基準文書との差分

現時点で `Plan.md`・`Implementation.md`・`Implementation-Roadmap.md` を変更する必要はない。実装中に基準文書と矛盾する判断が必要になった場合は、この表へ追記し、基準文書の改訂は別途承認を得る。

| 検出日 | 対象 | 差分 | 対応 |
|---|---|---|---|
| 2026-09-19 | Implementation 14.3 | API名は設計上の名前でCandid未固定 | `api-contract.md` に明記。Phase 1で実Candidを固定 |
| 2026-09-19 | Implementation 3.1 | `Cargo.toml`・`icp.yaml` は未作成だった | Phase 0で雛形を追加 |
| 2026-09-19 | ロードマップ4章 | 成果物は文書だが、12章-2のworkspace準備も本Phaseで実施 | 本README 2節に追記 |
| 2026-09-19 | Implementation 14.3 | Agent世代作成と履歴参照のAPI名が未定義（7章のフローと画面要件に必須） | `api-contract.md` 5.1に追加一覧（権限を拡大しない操作のみ） |
| 2026-09-19 | Implementation 3.1・14.3 | `policy_registry` のAPIが未定義 | `api-contract.md` 5節に4メソッドを設計名として定義 |
| 2026-09-19 | ロードマップ10.5 | チャートはLightweight Chartsを継続。Advanced Chartsは未評価のopen item | `ui-spec.md` 8節に必要機能と未評価理由を記録 |

## 7. 未決事項台帳

Phase 0の完了条件ではないが、並行して追跡する（ロードマップ4章）。

| ID | 項目 | 現状 | 影響 |
|---|---|---|---|
| O-1 | 運営主体、対象国、規約適合性 | 未調査 | 実顧客の募集・受付を許可しない |
| O-2 | SNSトークン配分・販売条件 | 未確定 | 本番開始条件 |
| O-3 | Advanced Chartsの提供条件・統合費用 | 未評価 | 初期必須機能に高度描画が必要になった場合のみ再判断（`ui-spec.md` 8節） |
| O-4 | testnet/mainnet Canister ID、署名鍵ID | 未確定 | `environments.md` に記録先のみ定義 |
| O-5 | ローカルネットワーク起動可否（icp launcher） | Phase 0で実測 | 実測結果を `../implementation-status.md` に記録 |

## 8. 明示しておく制限

- Canisterコード、Candid、testnet Canister IDはまだ存在しない。本ディレクトリの契約は実装の代替ではない。
- UIデモ（`frontend/`）は合成データ専用である。本ディレクトリの画面仕様は実装指示であり、現行デモが仕様を満たしている証明ではない。
- 機密性・相関耐性・資金安全性は、どの文書を書いても成立しない。Phase 1の実測と独立監査を要する。
