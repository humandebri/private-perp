# private-perp

機密資金管理＋Hyperliquid取引を一つの ICP Canister にまとめ、TanStack Start UI から接続するアプリです。

現在の配置・権限・検証手順は [単一 Canister 化](docs/phase-3/single-canister.md) を参照してください。旧設計書の五 Canister・SNS/guard 構成は公開試験版の配置には適用しません。

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

現行の統合 Wasm の送金・Agent承認・注文送信はPocketICと模擬HL応答で検証しています。旧5 Canister構成ではHL testnetの読取りまで確認しましたが、統合Wasmからの実HLへの送金・注文POST、公開ICPへの配置、実資金往復は未検証です。共通保管口座は直接送金の紐付けを減らしますが、[合成相関評価](docs/phase-3/privacy-local-eval.md)では現行B0のtop-1対応付け成功率が100%で、匿名性の基準には未達です。
