# private-perp

機密資金管理＋Hyperliquid取引の設計と、TanStack Start/Cloudflare Workers向けUIプロトタイプ。

- 実装：`frontend/`（合成データ専用）、`crates/`（ICP Canister。ローカル検証済み）
- 起動・試験・制限：`frontend/README.md`
- 要件：`Plan.md`
- 技術設計：`Implementation.md`
- Phase計画：`Implementation-Roadmap.md`
- 判断記録：`docs/adr/`（6本）
- Phase 0の契約・画面仕様：`docs/phase-0/README.md`
- 完了範囲と残件：`docs/implementation-status.md`

ICPバックエンド（`crates/`）は雛形ではなく、資金・署名・認証を担うCanisterを実装済みです。`funds_vault`（認証・セッション・複式台帳・予約・outboxの署名送信と照合・入金計上・回収・HPKE・Agent承認要求）、`trading_core`（認可境界・注文受付・取消送信・Agent鍵署名・照合）、`policy_registry`、`control_guard` が該当します。

検証はローカルに限られます。ホストテストとPocketIC統合試験（21ファイル・60試験）は成功していますが、**testnet・mainnetでの動作は未検証**であり、実資金の受付も行っていません。本番デプロイ、SNS移管、ウォレット接続も未実施です。UIのデモとローカル試験の成功を、本番の資金安全性やプライバシーの実証と解釈しないでください。
