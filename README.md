# private-perp

機密資金管理＋Hyperliquid取引の設計と、TanStack Start/Cloudflare Workers向けUIプロトタイプ。

- 実装：`frontend/`（合成データ専用）、`crates/`（Phase 0の雛形）
- 起動・試験・制限：`frontend/README.md`
- 要件：`Plan.md`
- 技術設計：`Implementation.md`
- Phase計画：`Implementation-Roadmap.md`
- 判断記録：`docs/adr/`（6本）
- Phase 0の契約・画面仕様：`docs/phase-0/README.md`
- 完了範囲と残件：`docs/implementation-status.md`

資金・署名・認証のICPバックエンドはまだ存在しません。`crates/` にあるのは `version` とDB初期化だけのCanister雛形であり、資金・署名・注文は未実装です。UIのデモを本番の資金安全性やプライバシーの実証と解釈しないでください。本番デプロイ、SNS移管、実資金の受付は行っていません。
