# private-perp

ICP の単一 `private_perp` Canister に資金保管・取引・policy・journal をまとめ、TanStack Start UI から Hyperliquid testnet を利用する試験用アプリです。[構成と権限](docs/phase-3/single-canister.md)を参照してください。

## 現在の状態（2026-09-29）

| 対象 | 状態 |
| --- | --- |
| ローカルコード | HTTP outcall v2 の非複製通信と、未完了処理がないときにワーカーを止める方式を実装済み。PocketIC・ブラウザ E2E 等で検証済み |
| 公開 IC Canister | `xis3j-paaaa-aaaai-axumq-cai` は cycles 補充待ちで停止中。ローカルの最新 Wasm は未反映 |
| [公開 HL testnet UI](https://private-perp-ui-testnet.hude.workers.dev) | UI は公開中だが、停止中の Canister を使う操作は現在できない |
| 実 HL testnet | 実送金、注文 POST、資金往復は未検証 |

cycles 補充、Canister のアップグレードと起動、公開 UI との再接続確認が必要です。[公開環境の受入記録](docs/phase-3/testnet-acceptance.md)と[ローカルのデプロイ前検証](docs/phase-3/predeploy-validation-2026-09-29.md)に結果と未検証範囲を分けて記録しています。

## 構成とローカル検証

- `crates/`：単一 Canister にリンクする資金、取引、policy、journal の実装
- `frontend/`：認証、保管残高、注文・建玉・履歴を扱う UI。起動・設定・単体試験は[UI README](frontend/README.md)
- `tools/mock-hl/`：ローカル試験用の Hyperliquid 模擬 API

ローカル IC と mock HL を起動し、実 Canister を通るブラウザ E2E を実行するには、リポジトリのルートで次を実行します。事前に `icp`、Rust、Node.js、pnpm、Playwright を用意してください。

```sh
bash scripts/local-e2e.sh
```

PocketIC を含む Rust 検証は `bash scripts/pocket-ic-test.sh --no-fail-fast`、単一 Canister のビルド・Candid 照合は `bash scripts/test-single-canister.sh` です。必要なツールと詳細は[単一 Canister 化](docs/phase-3/single-canister.md)を参照してください。ローカルの mock 試験合格は、公開 IC や実 HL での送金・注文成功を保証しません。

## 現行の仕様と制約

- [単一 Canister の構成と権限](docs/phase-3/single-canister.md)
- [共通保管口座の実装と制限](docs/phase-3/shared-reserve.md)
- [市況の取得・判定](docs/phase-3/market-consensus.md)
- [相関評価](docs/phase-3/privacy-local-eval.md)

共通保管口座は直接送金の紐付けを減らしますが、現行 B0 の合成相関評価では top-1 対応付け成功率が 100% で、匿名性の基準には達していません。資金往復の実装と匿名性の達成は別に評価します。
