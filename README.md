# private-perp

機密資金管理＋Hyperliquid取引を一つの ICP Canister にまとめ、TanStack Start UI から接続するアプリです。

現在の配置・権限・検証手順は [単一 Canister 化](docs/phase-3/single-canister.md) を参照してください。旧設計書の五 Canister・SNS/guard 構成は公開試験版の配置には適用しません。

2026-09-29現在、公開Canisterは cycles 補充待ちで停止中です。市場観測の合意方式の修正はローカル検証済みですが、アップグレードは残高不足で未反映です。[最新の受入記録](docs/phase-3/testnet-acceptance.md)を参照してください。

- 実装：`frontend/`（ローカル実接続）、`crates/`（ICP Canister）、`tools/mock-hl/`（ローカルvenue）
- 起動・試験・制限：`frontend/README.md`
- 要件：`Plan.md`
- 技術設計：`Implementation.md`
- Phase計画：`Implementation-Roadmap.md`
- 判断記録：`docs/adr/`（6本）
- Phase 0の契約・画面仕様：`docs/phase-0/README.md`
- 完了範囲と残件：`docs/implementation-status.md`
- 共通入金口座の実装と制限：`docs/phase-3/shared-reserve.md`

ICPバックエンド（`crates/`）は、`private_perp` 一つの Canister に資金・取引・policy・journal のモジュールを統合しています。資金側は認証・複式台帳・予約・送金と照合、取引側は注文・Agent署名・照合を担います。旧 `control_guard` は現行の統合 Wasm に含めません。

統合 Wasm は公開 ICP の `xis3j-paaaa-aaaai-axumq-cai` に配置し、[HL testnet UI](https://private-perp-ui-testnet.hude.workers.dev) を公開しています。署名ログイン、利用資格登録、入金先表示まで公開環境で確認しました。実 HL への送金・注文 POST、資金往復は未検証で、市場データ取得は IC ノード間の応答不一致により新規注文を停止しています。詳細は[受入記録](docs/phase-3/testnet-acceptance.md)。共通保管口座は直接送金の紐付けを減らしますが、[合成相関評価](docs/phase-3/privacy-local-eval.md)では現行B0のtop-1対応付け成功率が100%で、匿名性の基準には未達です。
